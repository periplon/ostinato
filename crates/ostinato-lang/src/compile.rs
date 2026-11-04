use std::collections::{BTreeMap, BTreeSet};

use ostinato_domain::{
    AgentSpec, BranchSpec, CycleSpec, ExecSpec, GatewaySpec, ImproveSpec, JoinMode, RetryPolicy,
    SplitMode, StepKind, StepSpec, WorkflowSpec, build_plan,
};
use serde_json::{Map, Value as Json};

use crate::condition::{reference_roots, step_references, validate_condition};
use crate::error::LangError;
use crate::eval::{TypeKind, Value, eval_file, is_hidden_key, value_from_json};
use crate::parser::parse;

type Struct = BTreeMap<String, Value>;

/// A parameter accepted by a workflow.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub default: Option<Json>,
    pub kind: String,
    pub required: bool,
}

/// Result of parsing, evaluating and validating a workflow file.
#[derive(Debug, Clone)]
pub struct CompiledWorkflow {
    pub spec: WorkflowSpec,
    pub params: Vec<ParamSpec>,
}

impl CompiledWorkflow {
    /// Merge CLI overrides with defaults and fail on missing required params.
    ///
    /// # Errors
    /// Returns [`LangError::Param`] for unknown or missing parameters.
    pub fn resolve_params(&self, overrides: &BTreeMap<String, String>) -> Result<Json, LangError> {
        let mut values = Map::new();
        for param in &self.params {
            if let Some(default) = &param.default {
                values.insert(param.name.clone(), default.clone());
            }
        }
        for (name, raw) in overrides {
            if !self.params.iter().any(|param| param.name == *name) {
                return Err(LangError::Param {
                    name: name.clone(),
                    message: "unknown parameter (declare it under `params`)".to_owned(),
                });
            }
            values.insert(name.clone(), parse_override(raw));
        }
        for param in &self.params {
            if param.required && !values.contains_key(&param.name) {
                return Err(LangError::Param {
                    name: param.name.clone(),
                    message: format!(
                        "missing required parameter of type `{}` (pass `-p {}=...`)",
                        param.kind, param.name
                    ),
                });
            }
        }
        Ok(Json::Object(values))
    }
}

/// Compile a CUE-compatible workflow source into a validated specification.
///
/// # Errors
/// Returns parse/eval/workflow errors, including invalid cycles.
pub fn compile(src: &str) -> Result<CompiledWorkflow, LangError> {
    let file = parse(src)?;
    let root = eval_file(&file)?;
    let params = extract_params_from_value(&root)?;
    let spec = compile_value(&root)?;
    Ok(CompiledWorkflow { spec, params })
}

/// Compile with concrete CLI overrides injected before evaluation, so that
/// compile-time interpolation sees the same values as runtime templates.
///
/// # Errors
/// Returns parse/eval/workflow errors.
pub fn compile_with_params(
    src: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<CompiledWorkflow, LangError> {
    let file = parse(src)?;
    let mut injected = BTreeMap::new();
    for (key, raw) in overrides {
        let trimmed = raw.trim();
        let value = if trimmed.starts_with('{')
            || trimmed.starts_with('[')
            || trimmed.starts_with('"')
            || matches!(trimmed, "true" | "false" | "null")
            || trimmed.parse::<f64>().is_ok()
        {
            serde_json::from_str::<Json>(trimmed)
                .ok()
                .map_or_else(|| Value::String(raw.clone()), |json| value_from_json(&json))
        } else {
            Value::String(raw.clone())
        };
        injected.insert(key.clone(), value);
    }
    let root = crate::eval::eval_file_with_overrides(&file, &injected)?;
    let params = extract_params_from_value(&root)?;
    let spec = compile_value(&root)?;
    Ok(CompiledWorkflow { spec, params })
}

/// Compile an already-evaluated value into a validated workflow specification.
///
/// # Errors
/// Returns workflow errors for missing/invalid fields and cycles.
pub fn compile_value(root: &Value) -> Result<WorkflowSpec, LangError> {
    let Value::Struct(root) = root else {
        return Err(LangError::Workflow(
            "top-level value must be a struct".to_owned(),
        ));
    };
    let workflow = root
        .get("workflow")
        .ok_or_else(|| LangError::Workflow("missing top-level `workflow` struct".to_owned()))?;
    let spec = extract_workflow(workflow)?;
    validate_gateway_cross_references(&spec)?;
    validate_gateway_condition_scopes(&spec)?;
    build_plan(&spec).map_err(|err| LangError::Workflow(err.to_string()))?;
    validate_conditions(&spec, &dummy_params(root))?;
    Ok(spec)
}

/// Compile a concrete JSON export produced by the official CUE CLI.
///
/// # Errors
/// Returns workflow errors for missing/invalid fields and cycles.
pub fn compile_json(root: &Json) -> Result<WorkflowSpec, LangError> {
    compile_value(&value_from_json(root))
}

fn extract_params_from_value(root: &Value) -> Result<Vec<ParamSpec>, LangError> {
    let Value::Struct(root) = root else {
        return Err(LangError::Workflow(
            "top-level value must be a struct".to_owned(),
        ));
    };
    extract_params(root)
}

fn parse_override(raw: &str) -> Json {
    let trimmed = raw.trim();
    let looks_like_json = trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('"')
        || matches!(trimmed, "true" | "false" | "null")
        || trimmed.parse::<f64>().is_ok();
    if looks_like_json {
        return serde_json::from_str(trimmed).unwrap_or_else(|_| Json::String(raw.to_owned()));
    }
    Json::String(raw.to_owned())
}

fn extract_params(root: &Struct) -> Result<Vec<ParamSpec>, LangError> {
    let Some(value) = root.get("params") else {
        return Ok(Vec::new());
    };
    let map = expect_struct(value, "params")?;
    let mut params = Vec::with_capacity(map.len());
    for (raw_name, value) in map {
        if raw_name.starts_with('\u{3}') {
            continue;
        }
        let name = raw_name
            .strip_prefix('\u{4}')
            .unwrap_or(raw_name)
            .to_owned();
        match value {
            Value::Type(kind) => params.push(ParamSpec {
                name: name.clone(),
                default: None,
                kind: type_kind_name(*kind).to_owned(),
                required: true,
            }),
            Value::Bottom => params.push(ParamSpec {
                name: name.clone(),
                default: None,
                kind: "any".to_owned(),
                required: true,
            }),
            concrete => {
                let default = concrete.to_json().ok_or_else(|| LangError::Param {
                    name: name.clone(),
                    message: "parameter default is not concrete".to_owned(),
                })?;
                params.push(ParamSpec {
                    name: name.clone(),
                    default: Some(default),
                    kind: concrete.type_name().to_owned(),
                    required: false,
                });
            }
        }
    }
    Ok(params)
}

fn extract_workflow(value: &Value) -> Result<WorkflowSpec, LangError> {
    let map = expect_struct(value, "workflow")?;
    reject_unknown(map, "workflow", &["name", "maxParallel", "steps"])?;
    let name = opt_string(map, "name", "workflow.name")?.unwrap_or_else(|| "workflow".to_owned());
    let max_parallel = opt_int(map, "maxParallel", "workflow.maxParallel")?.unwrap_or(4);
    if max_parallel < 1 {
        return Err(LangError::Workflow(
            "workflow.maxParallel must be >= 1".to_owned(),
        ));
    }
    if max_parallel > 256 {
        return Err(LangError::Workflow(
            "workflow.maxParallel must be <= 256".to_owned(),
        ));
    }
    let max_parallel = usize::try_from(max_parallel)
        .map_err(|_| LangError::Workflow("workflow.maxParallel is too large".to_owned()))?;

    let steps_value = map
        .get("steps")
        .ok_or_else(|| LangError::Workflow("workflow.steps is required".to_owned()))?;
    let steps_map = expect_struct(steps_value, "workflow.steps")?;
    if steps_map.keys().all(|key| is_hidden_key(key)) {
        return Err(LangError::Workflow(
            "workflow.steps must contain at least one step".to_owned(),
        ));
    }

    let mut steps = BTreeMap::new();
    for (step_name, step_value) in steps_map {
        if is_hidden_key(step_name) {
            continue;
        }
        let step = extract_step(step_name, step_value)?;
        steps.insert(step_name.clone(), step);
    }

    Ok(WorkflowSpec {
        name,
        max_parallel,
        steps,
    })
}

fn extract_step(name: &str, value: &Value) -> Result<StepSpec, LangError> {
    let path = format!("workflow.steps.{name}");
    let map = expect_struct(value, &path)?;
    reject_unknown(
        map,
        &path,
        &[
            "kind",
            "needs",
            "retry",
            "cycle",
            "exec",
            "agent",
            "improve",
            "gateway",
            "continueOnError",
            "timeoutMs",
        ],
    )?;
    let kind_name =
        opt_string(map, "kind", &format!("{path}.kind"))?.unwrap_or_else(|| "exec".to_owned());
    let kind = match kind_name.as_str() {
        "exec" => StepKind::Exec,
        "agent" => StepKind::Agent,
        "improve" => StepKind::Improve,
        "noop" => StepKind::Noop,
        "gateway" => StepKind::Gateway,
        other => {
            return Err(LangError::Workflow(format!(
                "{path}.kind: unknown kind `{other}` (expected exec, agent, improve, noop or gateway)"
            )));
        }
    };

    let needs = match map.get("needs").map(Value::concretize) {
        None => Vec::new(),
        Some(Value::String(need)) => vec![need.clone()],
        Some(Value::OpenList(_)) => Vec::new(),
        Some(Value::List(list) | Value::OpenListPrefix(list, _)) => {
            let mut needs = Vec::with_capacity(list.len());
            for item in list {
                needs.push(as_string(item, &format!("{path}.needs"))?);
            }
            needs
        }
        Some(other) => {
            return Err(LangError::Workflow(format!(
                "{path}.needs must be a string or list of strings, found {}",
                other.type_name()
            )));
        }
    };

    let gateway = extract_gateway(map.get("gateway"), &format!("{path}.gateway"), &needs)?;
    if kind == StepKind::Gateway {
        if gateway.is_none() {
            return Err(LangError::Workflow(format!(
                "{path}.gateway is required when kind = \"gateway\""
            )));
        }
        for field in [
            "retry",
            "cycle",
            "exec",
            "agent",
            "improve",
            "timeoutMs",
            "continueOnError",
        ] {
            if map.contains_key(field) {
                return Err(LangError::Workflow(format!(
                    "{path}.{field} is not allowed when kind = \"gateway\""
                )));
            }
        }
        if gateway.as_ref().is_some_and(|spec| spec.join.is_some()) && needs.is_empty() {
            return Err(LangError::Workflow(format!(
                "{path}.needs must not be empty for a join gateway"
            )));
        }
    } else if gateway.is_some() {
        return Err(LangError::Workflow(format!(
            "{path}.gateway is only allowed when kind = \"gateway\""
        )));
    }

    let is_gateway = kind == StepKind::Gateway;
    let retry = if is_gateway {
        RetryPolicy::default()
    } else {
        extract_retry(map.get("retry"), &format!("{path}.retry"))?
    };
    let cycle = if is_gateway {
        None
    } else {
        extract_cycle(map.get("cycle"), &format!("{path}.cycle"))?
    };
    let timeout_ms = if is_gateway {
        None
    } else {
        opt_int(map, "timeoutMs", &format!("{path}.timeoutMs"))?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    LangError::Workflow(format!("{path}.timeoutMs must be non-negative"))
                })
            })
            .transpose()?
    };
    let continue_on_error = if is_gateway {
        false
    } else {
        opt_bool(map, "continueOnError", &format!("{path}.continueOnError"))?.unwrap_or(false)
    };

    let exec = match map.get("exec").map(Value::concretize) {
        Some(value) => Some(extract_exec(value, &format!("{path}.exec"))?),
        None => None,
    };
    let agent = match map.get("agent").map(Value::concretize) {
        Some(value) => Some(extract_agent(value, &format!("{path}.agent"))?),
        None => None,
    };
    let improve = match map.get("improve").map(Value::concretize) {
        Some(value) => Some(extract_improve(value, &format!("{path}.improve"))?),
        None => None,
    };

    match kind {
        StepKind::Exec => {
            let spec = exec.as_ref().ok_or_else(|| {
                LangError::Workflow(format!("{path}.exec is required when kind = \"exec\""))
            })?;
            if spec.cmd.is_empty() {
                return Err(LangError::Workflow(format!(
                    "{path}.exec.cmd must not be empty"
                )));
            }
        }
        StepKind::Agent => {
            let spec = agent.as_ref().ok_or_else(|| {
                LangError::Workflow(format!("{path}.agent is required when kind = \"agent\""))
            })?;
            if spec.prompt.trim().is_empty() {
                return Err(LangError::Workflow(format!(
                    "{path}.agent.prompt must not be empty"
                )));
            }
        }
        StepKind::Improve => {
            if improve.is_none() {
                return Err(LangError::Workflow(format!(
                    "{path}.improve is required when kind = \"improve\""
                )));
            }
        }
        StepKind::Noop | StepKind::Gateway => {}
    }

    Ok(StepSpec {
        name: name.to_owned(),
        kind,
        needs,
        retry,
        cycle,
        exec,
        agent,
        improve,
        gateway,
        continue_on_error,
        timeout_ms,
    })
}

fn extract_gateway(
    value: Option<&Value>,
    path: &str,
    needs: &[String],
) -> Result<Option<GatewaySpec>, LangError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let map = expect_struct(value, path)?;
    reject_unknown(
        map,
        path,
        &[
            "split",
            "join",
            "branches",
            "default",
            "n",
            "expression",
            "labels",
        ],
    )?;

    let split = match opt_string(map, "split", &format!("{path}.split"))? {
        None => None,
        Some(mode) => Some(match mode.as_str() {
            "and" => SplitMode::And,
            "or" => SplitMode::Or,
            "xor" => SplitMode::Xor,
            "n_of" | "expression" => {
                return Err(LangError::Workflow(format!(
                    "{path}.split: `{mode}` is a join mode, not a split mode"
                )));
            }
            other => {
                return Err(LangError::Workflow(format!(
                    "{path}.split: unknown split mode `{other}` (expected and, or or xor)"
                )));
            }
        }),
    };
    let join = match opt_string(map, "join", &format!("{path}.join"))? {
        None => None,
        Some(mode) => Some(match mode.as_str() {
            "and" => JoinMode::And,
            "or" => JoinMode::Or,
            "xor" => JoinMode::Xor,
            "n_of" => JoinMode::NOf,
            "expression" => JoinMode::Expression,
            other => {
                return Err(LangError::Workflow(format!(
                    "{path}.join: unknown join mode `{other}` (expected and, or, xor, n_of or expression)"
                )));
            }
        }),
    };
    match (split, join) {
        (Some(_), Some(_)) => {
            return Err(LangError::Workflow(format!(
                "{path}: exactly one of `split` and `join` is allowed"
            )));
        }
        (None, None) => {
            return Err(LangError::Workflow(format!(
                "{path}: one of `split` or `join` is required"
            )));
        }
        _ => {}
    }

    let branches_declared = map.contains_key("branches");
    let default_declared = map.contains_key("default");
    let default = opt_string(map, "default", &format!("{path}.default"))?;
    let n = match opt_int(map, "n", &format!("{path}.n"))? {
        None => None,
        Some(value) => Some(
            u32::try_from(value)
                .map_err(|_| LangError::Workflow(format!("{path}.n must be a positive integer")))?,
        ),
    };
    let expression = opt_string(map, "expression", &format!("{path}.expression"))?;
    let mut labels = BTreeMap::new();
    if let Some(labels_value) = map.get("labels") {
        let label_map = expect_struct(labels_value, &format!("{path}.labels"))?;
        for (label, value) in label_map {
            if is_hidden_key(label) {
                continue;
            }
            labels.insert(
                label.clone(),
                as_string(value, &format!("{path}.labels.{label}"))?,
            );
        }
    }
    let mut branches = BTreeMap::new();
    if let Some(branches_value) = map.get("branches") {
        let branch_map = expect_struct(branches_value, &format!("{path}.branches"))?;
        for (branch, branch_value) in branch_map {
            if is_hidden_key(branch) {
                continue;
            }
            let branch_path = format!("{path}.branches.{branch}");
            let condition = match branch_value.concretize() {
                Value::Bool(true) => None,
                Value::Bool(false) => {
                    return Err(LangError::Workflow(format!(
                        "{branch_path} must be `true` or a condition string; `false` branches are not allowed"
                    )));
                }
                Value::String(condition) => {
                    if condition.trim().is_empty() {
                        return Err(LangError::Workflow(format!(
                            "{branch_path} must not be an empty condition"
                        )));
                    }
                    Some(condition.clone())
                }
                other => {
                    return Err(LangError::Workflow(format!(
                        "{branch_path} must be `true` or a condition string, found {}",
                        other.type_name()
                    )));
                }
            };
            branches.insert(branch.clone(), BranchSpec { condition });
        }
    }

    if let Some(split_mode) = split {
        if n.is_some() || expression.is_some() || !labels.is_empty() {
            return Err(LangError::Workflow(format!(
                "{path}: `n`, `expression` and `labels` are only allowed on join gateways"
            )));
        }
        if branches.is_empty() {
            return Err(LangError::Workflow(format!(
                "{path}.branches must contain at least one branch for a split gateway"
            )));
        }
        if split_mode == SplitMode::And && default.is_some() {
            let branch = default.clone().unwrap_or_default();
            return Err(LangError::Workflow(format!(
                "{path}.default is not allowed on an AND split (branch `{branch}`)"
            )));
        }
        if let Some(branch) = &default
            && !branches.contains_key(branch)
        {
            return Err(LangError::Workflow(format!(
                "{path}.default `{branch}` is not one of the declared branches"
            )));
        }
        for (branch, branch_spec) in &branches {
            let branch_path = format!("{path}.branches.{branch}");
            let is_default = default.as_deref() == Some(branch.as_str());
            match split_mode {
                SplitMode::And => {
                    if branch_spec.condition.is_some() {
                        return Err(LangError::Workflow(format!(
                            "{branch_path} must be unconditional `true` on an AND split"
                        )));
                    }
                }
                SplitMode::Xor => {
                    if is_default {
                        if branch_spec.condition.is_some() {
                            return Err(LangError::Workflow(format!(
                                "{branch_path} is the default branch and must be `true`"
                            )));
                        }
                    } else if branch_spec.condition.is_none() {
                        return Err(LangError::Workflow(format!(
                            "{branch_path} requires a condition on an XOR split"
                        )));
                    }
                }
                SplitMode::Or => {
                    if is_default && branch_spec.condition.is_some() {
                        return Err(LangError::Workflow(format!(
                            "{branch_path} is the default branch and must be `true`"
                        )));
                    }
                }
            }
        }
    }

    if let Some(join_mode) = join {
        if branches_declared || default_declared {
            return Err(LangError::Workflow(format!(
                "{path}: a join gateway must not declare `branches` or `default`"
            )));
        }
        match join_mode {
            JoinMode::NOf => {
                let Some(required) = n else {
                    return Err(LangError::Workflow(format!(
                        "{path}.n is required when join = \"n_of\""
                    )));
                };
                if expression.is_some() || !labels.is_empty() {
                    return Err(LangError::Workflow(format!(
                        "{path}: `expression` and `labels` are only allowed when join = \"expression\""
                    )));
                }
                let mut distinct = BTreeSet::new();
                for need in needs {
                    if !distinct.insert(need.as_str()) {
                        return Err(LangError::Workflow(format!(
                            "{path}.needs contains duplicate `{need}`; `n_of` requires distinct inputs"
                        )));
                    }
                }
                if required as usize > distinct.len() || required == 0 {
                    return Err(LangError::Workflow(format!(
                        "{path}.n must be between 1 and {}",
                        distinct.len()
                    )));
                }
            }
            JoinMode::Expression => {
                if expression
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(LangError::Workflow(format!(
                        "{path}.expression is required when join = \"expression\""
                    )));
                }
                if n.is_some() {
                    return Err(LangError::Workflow(format!(
                        "{path}.n is only allowed when join = \"n_of\""
                    )));
                }
                let needs_set: BTreeSet<&str> = needs.iter().map(String::as_str).collect();
                let mut targets = BTreeSet::new();
                for (label, target) in &labels {
                    if !valid_label(label) {
                        return Err(LangError::Workflow(format!(
                            "{path}.labels.{label} must match [A-Za-z_][A-Za-z0-9_]* and must not be a condition keyword"
                        )));
                    }
                    if !needs_set.contains(target.as_str()) {
                        return Err(LangError::Workflow(format!(
                            "{path}.labels.{label} targets `{target}`, which is not in `needs`"
                        )));
                    }
                    if !targets.insert(target.as_str()) {
                        return Err(LangError::Workflow(format!(
                            "{path}.labels maps multiple labels to `{target}`; labels must be a bijection"
                        )));
                    }
                }
                if !labels.is_empty() && labels.len() != needs_set.len() {
                    return Err(LangError::Workflow(format!(
                        "{path}.labels must cover every distinct `needs` entry exactly once"
                    )));
                }
            }
            JoinMode::And | JoinMode::Or | JoinMode::Xor => {
                if n.is_some() || expression.is_some() || !labels.is_empty() {
                    return Err(LangError::Workflow(format!(
                        "{path}: `n` is only allowed for `n_of`; `expression`/`labels` only for `expression`"
                    )));
                }
            }
        }
    }

    Ok(Some(GatewaySpec {
        split,
        join,
        branches,
        default,
        n,
        expression,
        labels,
    }))
}

fn valid_label(label: &str) -> bool {
    if matches!(label, "and" | "or" | "not" | "true" | "false" | "null") {
        return false;
    }
    let mut chars = label.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn extract_retry(value: Option<&Value>, path: &str) -> Result<RetryPolicy, LangError> {
    let Some(value) = value else {
        return Ok(RetryPolicy::default());
    };
    if let Value::Int(max_retries) = value.concretize() {
        let max_retries = u32::try_from(*max_retries)
            .map_err(|_| LangError::Workflow(format!("{path} must be non-negative")))?;
        if max_retries > 100 {
            return Err(LangError::Workflow(format!("{path} must be <= 100")));
        }
        return Ok(RetryPolicy {
            max_retries,
            ..RetryPolicy::default()
        });
    }
    if let Value::BigInt(max_retries) = value.concretize() {
        let max_retries = u32::try_from(max_retries)
            .map_err(|_| LangError::Workflow(format!("{path} must be non-negative and <= 100")))?;
        if max_retries > 100 {
            return Err(LangError::Workflow(format!("{path} must be <= 100")));
        }
        return Ok(RetryPolicy {
            max_retries,
            ..RetryPolicy::default()
        });
    }
    let map = expect_struct(value, path)?;
    reject_unknown(map, path, &["max", "baseMs", "factor", "maxMs", "jitter"])?;
    let max_retries = opt_int(map, "max", &format!("{path}.max"))?.unwrap_or(0);
    let max_retries = u32::try_from(max_retries)
        .map_err(|_| LangError::Workflow(format!("{path}.max must be non-negative")))?;
    if max_retries > 100 {
        return Err(LangError::Workflow(format!("{path}.max must be <= 100")));
    }
    let base_delay_ms = opt_int(map, "baseMs", &format!("{path}.baseMs"))?.unwrap_or(100);
    let base_delay_ms = u64::try_from(base_delay_ms)
        .map_err(|_| LangError::Workflow(format!("{path}.baseMs must be non-negative")))?;
    let factor = opt_float(map, "factor", &format!("{path}.factor"))?.unwrap_or(2.0);
    if factor < 1.0 {
        return Err(LangError::Workflow(format!("{path}.factor must be >= 1.0")));
    }
    let max_delay_ms = opt_int(map, "maxMs", &format!("{path}.maxMs"))?.unwrap_or(30_000);
    let max_delay_ms = u64::try_from(max_delay_ms)
        .map_err(|_| LangError::Workflow(format!("{path}.maxMs must be non-negative")))?;
    let jitter = opt_bool(map, "jitter", &format!("{path}.jitter"))?.unwrap_or(true);
    Ok(RetryPolicy {
        max_retries,
        base_delay_ms,
        factor,
        max_delay_ms,
        jitter,
    })
}

fn extract_cycle(value: Option<&Value>, path: &str) -> Result<Option<CycleSpec>, LangError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let map = expect_struct(value, path)?;
    reject_unknown(map, path, &["max", "until"])?;
    let max = opt_int(map, "max", &format!("{path}.max"))?.unwrap_or(3);
    let max_rounds = u32::try_from(max)
        .map_err(|_| LangError::Workflow(format!("{path}.max must be non-negative")))?;
    if max_rounds == 0 {
        return Err(LangError::Workflow(format!("{path}.max must be >= 1")));
    }
    if max_rounds > 1000 {
        return Err(LangError::Workflow(format!("{path}.max must be <= 1000")));
    }
    let until = opt_string(map, "until", &format!("{path}.until"))?;
    Ok(Some(CycleSpec { max_rounds, until }))
}

fn extract_exec(value: &Value, path: &str) -> Result<ExecSpec, LangError> {
    let map = expect_struct(value, path)?;
    reject_unknown(map, path, &["cmd", "cwd", "env", "timeoutMs"])?;
    let cmd_value = map
        .get("cmd")
        .ok_or_else(|| LangError::Workflow(format!("{path}.cmd is required")))?;
    let cmd = match cmd_value {
        Value::String(command) => vec!["sh".to_owned(), "-c".to_owned(), command.clone()],
        Value::OpenList(_) => Vec::new(),
        Value::List(list) | Value::OpenListPrefix(list, _) => {
            let mut cmd = Vec::with_capacity(list.len());
            for item in list {
                cmd.push(as_string(item, &format!("{path}.cmd"))?);
            }
            cmd
        }
        other => {
            return Err(LangError::Workflow(format!(
                "{path}.cmd must be a string or list of strings, found {}",
                other.type_name()
            )));
        }
    };
    let cwd = opt_string(map, "cwd", &format!("{path}.cwd"))?;
    let timeout_ms = opt_int(map, "timeoutMs", &format!("{path}.timeoutMs"))?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| LangError::Workflow(format!("{path}.timeoutMs must be non-negative")))
        })
        .transpose()?;
    let env = match map.get("env").map(Value::concretize) {
        None => BTreeMap::new(),
        Some(value) => {
            let env_map = expect_struct(value, &format!("{path}.env"))?;
            let mut env = BTreeMap::new();
            for (key, value) in env_map {
                if is_hidden_key(key) {
                    continue;
                }
                env.insert(key.clone(), as_string(value, &format!("{path}.env.{key}"))?);
            }
            env
        }
    };
    Ok(ExecSpec {
        cmd,
        cwd,
        env,
        timeout_ms,
    })
}

fn extract_agent(value: &Value, path: &str) -> Result<AgentSpec, LangError> {
    let map = expect_struct(value, path)?;
    reject_unknown(map, path, &["prompt", "system", "model", "command"])?;
    let prompt = opt_string(map, "prompt", &format!("{path}.prompt"))?
        .ok_or_else(|| LangError::Workflow(format!("{path}.prompt is required")))?;
    let system = opt_string(map, "system", &format!("{path}.system"))?;
    let model = opt_string(map, "model", &format!("{path}.model"))?;
    let command = match map.get("command").map(Value::concretize) {
        None => None,
        Some(Value::OpenList(_)) => Some(Vec::new()),
        Some(Value::List(list) | Value::OpenListPrefix(list, _)) => {
            let mut command = Vec::with_capacity(list.len());
            for item in list {
                command.push(as_string(item, &format!("{path}.command"))?);
            }
            Some(command)
        }
        Some(Value::String(command)) => {
            Some(vec!["sh".to_owned(), "-c".to_owned(), command.clone()])
        }
        Some(other) => {
            return Err(LangError::Workflow(format!(
                "{path}.command must be a string or list, found {}",
                other.type_name()
            )));
        }
    };
    Ok(AgentSpec {
        prompt,
        system,
        model,
        command,
    })
}

fn extract_improve(value: &Value, path: &str) -> Result<ImproveSpec, LangError> {
    let map = expect_struct(value, path)?;
    reject_unknown(map, path, &["prompt", "maxSamples"])?;
    let prompt = opt_string(map, "prompt", &format!("{path}.prompt"))?;
    let max_samples = opt_int(map, "maxSamples", &format!("{path}.maxSamples"))?.unwrap_or(20);
    let max_samples = usize::try_from(max_samples)
        .map_err(|_| LangError::Workflow(format!("{path}.maxSamples must be non-negative")))?;
    Ok(ImproveSpec {
        prompt,
        max_samples,
    })
}

fn reject_unknown(map: &Struct, path: &str, allowed: &[&str]) -> Result<(), LangError> {
    for key in map.keys() {
        if is_hidden_key(key) {
            continue;
        }
        if !allowed.contains(&key.as_str()) {
            return Err(LangError::Workflow(format!(
                "{path}: unknown field `{key}` (allowed: {})",
                allowed.join(", ")
            )));
        }
    }
    Ok(())
}

fn dummy_params(root: &Struct) -> Json {
    let Some(Value::Struct(params) | Value::Definition { fields: params, .. }) = root.get("params")
    else {
        return Json::Object(Map::new());
    };
    let mut out = Map::new();
    for (key, value) in params {
        if is_hidden_key(key) {
            continue;
        }
        out.insert(key.clone(), dummy_value(value));
    }
    Json::Object(out)
}

fn dummy_value(value: &Value) -> Json {
    match value {
        Value::String(_) => Json::String(String::new()),
        Value::Bool(_) => Json::Bool(false),
        Value::Int(_)
        | Value::BigInt(_)
        | Value::Float(_)
        | Value::Type(TypeKind::Int | TypeKind::Float | TypeKind::Number) => {
            Json::Number(serde_json::Number::from(0))
        }
        Value::List(_) | Value::OpenList(_) | Value::OpenListPrefix(_, _) => {
            Json::Array(Vec::new())
        }
        Value::Struct(map) | Value::Definition { fields: map, .. } => Json::Object(
            map.iter()
                .filter(|(key, _)| !is_hidden_key(key))
                .map(|(key, value)| (key.clone(), dummy_value(value)))
                .collect(),
        ),
        Value::Type(TypeKind::String) => Json::String(String::new()),
        Value::Type(TypeKind::Bool) => Json::Bool(false),
        Value::Type(TypeKind::Bytes) => Json::Null,
        Value::Disjunction(alternatives, defaults) => {
            let index = defaults.first().copied().unwrap_or(0);
            alternatives.get(index).map_or(Json::Null, dummy_value)
        }
        Value::Bytes(_)
        | Value::Bounds(_)
        | Value::Null
        | Value::Bottom
        | Value::Builtin(_)
        | Value::Conflict(_)
        | Value::Recursive(_)
        | Value::Deferred(_, _) => Json::Null,
    }
}

fn validate_gateway_cross_references(spec: &WorkflowSpec) -> Result<(), LangError> {
    let split_gateways: BTreeSet<&str> = spec
        .steps
        .iter()
        .filter(|(_, step)| {
            step.gateway
                .as_ref()
                .is_some_and(|gateway| gateway.split.is_some())
        })
        .map(|(name, _)| name.as_str())
        .collect();
    for (name, step) in &spec.steps {
        for need in &step.needs {
            if split_gateways.contains(need.as_str()) {
                return Err(LangError::Workflow(format!(
                    "workflow.steps.{name}.needs must not list split gateway `{need}`; branch edges are implicit and selected at runtime"
                )));
            }
        }
    }

    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split.is_none() {
            continue;
        }
        for branch in gateway.branches.keys() {
            let branch_path = format!("workflow.steps.{name}.gateway.branches.{branch}");
            let Some(target) = spec.steps.get(branch) else {
                return Err(LangError::Workflow(format!(
                    "{branch_path} targets unknown step `{branch}`; declare it under workflow.steps"
                )));
            };
            if branch == name {
                return Err(LangError::Workflow(format!(
                    "{branch_path} must not target the gateway itself"
                )));
            }
            if target.kind == StepKind::Gateway
                && target
                    .gateway
                    .as_ref()
                    .is_some_and(|spec| spec.join.is_some())
            {
                return Err(LangError::Workflow(format!(
                    "{branch_path} targets join gateway `{branch}`; joins cannot be branch targets"
                )));
            }
            if target.needs.iter().any(|need| need == name) {
                return Err(LangError::Workflow(format!(
                    "workflow.steps.{branch}.needs must not list split gateway `{name}`; the branch edge is implicit"
                )));
            }
            if !target.needs.is_empty() {
                return Err(LangError::Workflow(format!(
                    "workflow.steps.{branch}.needs must be empty; branch target of `{name}` cannot have prerequisites (put them on the gateway)"
                )));
            }
        }
    }
    Ok(())
}

fn validate_gateway_condition_scopes(spec: &WorkflowSpec) -> Result<(), LangError> {
    let mut incoming: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, step) in &spec.steps {
        for need in &step.needs {
            incoming
                .entry(name.clone())
                .or_default()
                .insert(need.clone());
        }
        if let Some(gateway) = &step.gateway
            && gateway.split.is_some()
        {
            for target in gateway.branches.keys() {
                incoming
                    .entry(target.clone())
                    .or_default()
                    .insert(name.clone());
            }
        }
    }
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split.is_none() {
            continue;
        }
        let ancestors = ancestors(&incoming, name);
        for (branch, branch_spec) in &gateway.branches {
            let Some(condition) = &branch_spec.condition else {
                continue;
            };
            for reference in step_references(condition) {
                if !ancestors.contains(&reference) {
                    return Err(LangError::Workflow(format!(
                        "workflow.steps.{name}.gateway.branches.{branch}: condition references `{reference}` which is not a transitive predecessor of the gateway; branch conditions may only use params, run_id, workflow, round, stepStatus and predecessor outputs"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn ancestors(incoming: &BTreeMap<String, BTreeSet<String>>, name: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = incoming.get(name).into_iter().flatten().cloned().collect();
    while let Some(current) = stack.pop() {
        if seen.insert(current.clone())
            && let Some(parents) = incoming.get(&current)
        {
            stack.extend(parents.iter().cloned());
        }
    }
    seen
}

fn validate_conditions(spec: &WorkflowSpec, params: &Json) -> Result<(), LangError> {
    let mut steps = Map::new();
    let mut step_status = Map::new();
    let mut step_round = Map::new();
    for name in spec.steps.keys() {
        steps.insert(name.clone(), serde_json::json!({}));
        step_status.insert(name.clone(), Json::String("succeeded".to_owned()));
        step_round.insert(name.clone(), Json::from(0));
    }
    let context = serde_json::json!({
        "run_id": "validate",
        "workflow": spec.name,
        "round": 0,
        "params": params,
        "steps": steps,
        "stepStatus": step_status,
        "stepRound": step_round,
    });
    for (name, step) in &spec.steps {
        if let Some(until) = step.cycle.as_ref().and_then(|cycle| cycle.until.as_ref()) {
            validate_condition(until, &context).map_err(|err| {
                LangError::Workflow(format!(
                    "workflow.steps.{name}.cycle.until is invalid: {err}"
                ))
            })?;
        }
        if let Some(gateway) = &step.gateway {
            for (branch, branch_spec) in &gateway.branches {
                if let Some(condition) = &branch_spec.condition {
                    validate_condition(condition, &context).map_err(|err| {
                        LangError::Workflow(format!(
                            "workflow.steps.{name}.gateway.branches.{branch} is invalid: {err}"
                        ))
                    })?;
                }
            }
            if let Some(expression) = &gateway.expression {
                for root in reference_roots(expression) {
                    if !matches!(
                        root.as_str(),
                        "edges" | "activeCount" | "deadCount" | "totalCount" | "true" | "false"
                    ) {
                        return Err(LangError::Workflow(format!(
                            "workflow.steps.{name}.gateway.expression may only reference edges, activeCount, deadCount and totalCount; found `{root}`"
                        )));
                    }
                }
                let mut edges = Map::new();
                if gateway.labels.is_empty() {
                    for need in &step.needs {
                        edges.insert(need.clone(), Json::String("active".to_owned()));
                    }
                } else {
                    for label in gateway.labels.keys() {
                        edges.insert(label.clone(), Json::String("active".to_owned()));
                    }
                }
                let mut edge_context = context.clone();
                if let Some(map) = edge_context.as_object_mut() {
                    map.insert("edges".to_owned(), Json::Object(edges));
                    map.insert("activeCount".to_owned(), Json::from(0));
                    map.insert("deadCount".to_owned(), Json::from(0));
                    map.insert("totalCount".to_owned(), Json::from(step.needs.len()));
                }
                validate_condition(expression, &edge_context).map_err(|err| {
                    LangError::Workflow(format!(
                        "workflow.steps.{name}.gateway.expression is invalid: {err}"
                    ))
                })?;
            }
        }
    }
    Ok(())
}

fn expect_struct<'a>(value: &'a Value, path: &str) -> Result<&'a Struct, LangError> {
    match value.concretize() {
        Value::Struct(map) | Value::Definition { fields: map, .. } => Ok(map),
        other => Err(LangError::Workflow(format!(
            "{path} must be a struct, found {}",
            other.type_name()
        ))),
    }
}

fn as_string(value: &Value, path: &str) -> Result<String, LangError> {
    match value.concretize() {
        Value::String(text) => Ok(text.clone()),
        other => Err(LangError::Workflow(format!(
            "{path} must be a string, found {}",
            other.type_name()
        ))),
    }
}

fn opt_string(map: &Struct, key: &str, path: &str) -> Result<Option<String>, LangError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => Ok(Some(as_string(value, path)?)),
    }
}

fn opt_int(map: &Struct, key: &str, path: &str) -> Result<Option<i64>, LangError> {
    match map.get(key).map(Value::concretize) {
        None => Ok(None),
        Some(Value::Int(value)) => Ok(Some(*value)),
        Some(Value::BigInt(value)) => i64::try_from(value)
            .map(Some)
            .map_err(|_| LangError::Workflow(format!("{path} must fit in an int"))),
        Some(other) => Err(LangError::Workflow(format!(
            "{path} must be an int, found {}",
            other.type_name()
        ))),
    }
}

fn opt_float(map: &Struct, key: &str, path: &str) -> Result<Option<f64>, LangError> {
    match map.get(key).map(Value::concretize) {
        None => Ok(None),
        Some(Value::Float(value)) => {
            Ok(Some(value.to_string().parse::<f64>().map_err(|_| {
                LangError::Workflow(format!("{path} must be a finite number"))
            })?))
        }
        Some(Value::Int(value)) => Ok(Some(*value as f64)),
        Some(Value::BigInt(value)) => {
            Ok(Some(value.to_string().parse::<f64>().map_err(|_| {
                LangError::Workflow(format!("{path} must be a finite number"))
            })?))
        }
        Some(other) => Err(LangError::Workflow(format!(
            "{path} must be a number, found {}",
            other.type_name()
        ))),
    }
}

fn opt_bool(map: &Struct, key: &str, path: &str) -> Result<Option<bool>, LangError> {
    match map.get(key).map(Value::concretize) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(other) => Err(LangError::Workflow(format!(
            "{path} must be a bool, found {}",
            other.type_name()
        ))),
    }
}

fn type_kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::String => "string",
        TypeKind::Int => "int",
        TypeKind::Float => "float",
        TypeKind::Number => "number",
        TypeKind::Bool => "bool",
        TypeKind::Bytes => "bytes",
    }
}
