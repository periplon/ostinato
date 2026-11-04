//! Pure gateway activation logic.
//!
//! This module contains no I/O and depends only on the domain model and the
//! pure condition interpreter. The engine persists the decisions it produces.

use std::collections::BTreeMap;

use ostinato_domain::{
    ActivationScope, CycleInfo, DomainError, EdgeOrigin, EdgeScope, GatewaySpec, JoinMode,
    PlanEdge, PlanNode, SplitMode, StepRecord, StepStatus, WorkflowSpec,
};
use ostinato_lang::eval_condition;
use serde_json::{Map, Value as Json, json};
use thiserror::Error;

/// What the scheduler should do with a plan node at a group boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activation {
    /// Execute the node (or evaluate a gateway).
    Active,
    /// The node or edge can no longer receive a token.
    Dead { reason: String },
    /// A gateway could not make a decision. The run must fail.
    Fail { error: DomainError },
}

/// State of one incoming edge at a group boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeState {
    Active,
    Dead,
    /// A round-0 carry edge: no token exists yet and the edge must not count
    /// as active or dead.
    Absent,
}

impl EdgeState {
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Errors that indicate a plan/checkpoint bug or an invalid gateway decision.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ActivationError {
    #[error("invalid activation state: {0}")]
    InvalidState(String),
    #[error("gateway `{gateway}` cannot be evaluated: {error}")]
    Gateway { gateway: String, error: DomainError },
}

/// One branch evaluation, persisted for the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEvaluation {
    pub condition: Option<String>,
    pub result: bool,
    pub default: bool,
}

/// Result of evaluating a split gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitDecision {
    pub chosen: Vec<String>,
    pub evaluations: BTreeMap<String, BranchEvaluation>,
    pub used_default: bool,
}

/// Derive the state of every incoming edge for a node.
///
/// # Errors
/// Returns [`ActivationError::InvalidState`] when a predecessor record is
/// missing or inconsistent with the persisted checkpoint.
pub fn incoming_edges(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
) -> Result<Vec<(PlanEdge, EdgeState)>, ActivationError> {
    incoming_edges_at(node, states, spec, node.round, None)
}

/// Derive incoming edge state for a specific round.
///
/// # Errors
/// Returns [`ActivationError::InvalidState`] when a predecessor record is
/// missing, stale or inconsistent.
pub fn incoming_edges_at(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    round: u32,
    cycle: Option<&CycleInfo>,
) -> Result<Vec<(PlanEdge, EdgeState)>, ActivationError> {
    let mut edges = Vec::with_capacity(node.incoming.len());
    for edge in &node.incoming {
        edges.push((edge.clone(), edge_state(edge, states, spec, round, cycle)?));
    }
    Ok(edges)
}

/// Decide how a node is activated from its persisted predecessor records.
///
/// This convenience wrapper is for tests and non-cycle nodes. The engine uses
/// [`decide_with_context`] or [`decide_round`].
///
/// # Errors
/// Returns [`ActivationError`] when the checkpoint does not contain enough
/// information to reconstruct the edge states.
pub fn decide(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
) -> Result<Activation, ActivationError> {
    decide_with_context(node, states, spec, &Json::Null)
}

/// Decide activation with a condition context (needed by expression joins).
///
/// # Errors
/// Returns [`ActivationError`] when the checkpoint is incomplete or an
/// expression join cannot be evaluated.
pub fn decide_with_context(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    context: &Json,
) -> Result<Activation, ActivationError> {
    if node.activation == ActivationScope::LegacyCycle {
        return Ok(Activation::Active);
    }
    if node.incoming.is_empty() {
        return Ok(Activation::Active);
    }
    decide_inner(node, states, spec, context, node.round, None, true)
}

/// Decide activation for a gateway-bearing cycle round.
///
/// # Errors
/// Returns [`ActivationError`] for stale/missing predecessor rows or an
/// invalid expression.
pub fn decide_round(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    context: &Json,
    cycle: &CycleInfo,
    entered: bool,
) -> Result<Activation, ActivationError> {
    decide_inner(
        node,
        states,
        spec,
        context,
        node.round,
        Some(cycle),
        entered,
    )
}

fn decide_inner(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    context: &Json,
    round: u32,
    cycle: Option<&CycleInfo>,
    entered: bool,
) -> Result<Activation, ActivationError> {
    if !entered {
        return Ok(Activation::Dead {
            reason: "cycle_not_entered".to_owned(),
        });
    }
    let edges = incoming_edges_at(node, states, spec, round, cycle)?;
    let active = edges.iter().filter(|(_, state)| state.is_active()).count();
    let dead = edges
        .iter()
        .filter(|(_, state)| matches!(state, EdgeState::Dead))
        .count();
    let absent = edges
        .iter()
        .filter(|(_, state)| matches!(state, EdgeState::Absent))
        .count();

    if let Some(gateway) = &node.gateway
        && let Some(mode) = gateway.join
    {
        return join_outcome(&node.step, gateway, mode, &edges, context);
    }

    if node.activation == ActivationScope::RoundCycle
        && round == 0
        && active == 0
        && dead == 0
        && absent > 0
    {
        let root_entry = cycle.is_some_and(|cycle| {
            cycle.entries.iter().any(|entry| entry == &node.step)
                && cycle.external_incoming.is_empty()
        });
        if root_entry
            && node
                .gateway
                .as_ref()
                .is_none_or(|gateway| gateway.join.is_none())
        {
            return Ok(Activation::Active);
        }
        return Ok(Activation::Dead {
            reason: "cycle_not_entered".to_owned(),
        });
    }

    // Normal steps and split gateways require every same-round prerequisite
    // edge to be active. Absent round-0 carry edges are ignored; if there are
    // no active edges at all, the node is dead.
    if active == 0 || dead > 0 {
        Ok(Activation::Dead {
            reason: "dead_path".to_owned(),
        })
    } else {
        Ok(Activation::Active)
    }
}

fn join_outcome(
    step: &str,
    gateway: &GatewaySpec,
    mode: JoinMode,
    edges: &[(PlanEdge, EdgeState)],
    context: &Json,
) -> Result<Activation, ActivationError> {
    let active: Vec<&PlanEdge> = edges
        .iter()
        .filter(|(_, state)| state.is_active())
        .map(|(edge, _)| edge)
        .collect();
    let dead: Vec<&PlanEdge> = edges
        .iter()
        .filter(|(_, state)| matches!(state, EdgeState::Dead))
        .map(|(edge, _)| edge)
        .collect();
    match mode {
        JoinMode::And => {
            if !dead.is_empty() || active.is_empty() {
                Ok(Activation::Dead {
                    reason: "dead_path".to_owned(),
                })
            } else {
                Ok(Activation::Active)
            }
        }
        JoinMode::Or => {
            if active.is_empty() {
                Ok(Activation::Dead {
                    reason: "dead_path".to_owned(),
                })
            } else {
                Ok(Activation::Active)
            }
        }
        JoinMode::Xor => match active.len() {
            0 => Ok(Activation::Dead {
                reason: "dead_path".to_owned(),
            }),
            1 => Ok(Activation::Active),
            _ => Ok(Activation::Fail {
                error: DomainError::GatewayAmbiguous(step.to_owned()),
            }),
        },
        JoinMode::NOf => {
            let Some(required) = gateway.n else {
                return Ok(Activation::Fail {
                    error: DomainError::GatewayQuorumMissingN(step.to_owned()),
                });
            };
            if active.is_empty() {
                Ok(Activation::Dead {
                    reason: "dead_path".to_owned(),
                })
            } else if active.len() < required as usize {
                Ok(Activation::Fail {
                    error: DomainError::GatewayQuorumNotMet {
                        gateway: step.to_owned(),
                        required,
                        active: active.len(),
                    },
                })
            } else {
                Ok(Activation::Active)
            }
        }
        JoinMode::Expression => {
            let expression =
                gateway
                    .expression
                    .as_deref()
                    .ok_or_else(|| ActivationError::Gateway {
                        gateway: step.to_owned(),
                        error: DomainError::GatewayExpressionMissing(step.to_owned()),
                    })?;
            let mut edge_values = Map::new();
            for (edge, state) in edges {
                let label = gateway
                    .labels
                    .iter()
                    .find_map(|(label, target)| (target == &edge.from).then_some(label.as_str()))
                    .unwrap_or(edge.from.as_str());
                edge_values.insert(label.to_owned(), Json::Bool(state.is_active()));
            }
            let mut edge_context = context.clone();
            if let Some(map) = edge_context.as_object_mut() {
                map.insert("edges".to_owned(), Json::Object(edge_values));
                map.insert(
                    "activeCount".to_owned(),
                    Json::from(u64::try_from(active.len()).unwrap_or(u64::MAX)),
                );
                map.insert(
                    "deadCount".to_owned(),
                    Json::from(u64::try_from(dead.len()).unwrap_or(u64::MAX)),
                );
                map.insert(
                    "totalCount".to_owned(),
                    Json::from(u64::try_from(edges.len()).unwrap_or(u64::MAX)),
                );
            }
            let result = eval_condition(expression, &edge_context).map_err(|err| {
                ActivationError::Gateway {
                    gateway: step.to_owned(),
                    error: DomainError::GatewayCondition {
                        gateway: step.to_owned(),
                        message: err.to_string(),
                    },
                }
            })?;
            if result {
                Ok(Activation::Active)
            } else {
                Ok(Activation::Dead {
                    reason: "expression_false".to_owned(),
                })
            }
        }
    }
}

/// Whether a cycle has a token for the given round.
///
/// # Errors
/// Returns [`ActivationError::InvalidState`] when an entry/carry row is stale
/// or missing.
pub fn cycle_entered(
    cycle: &CycleInfo,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    round: u32,
) -> Result<bool, ActivationError> {
    if round == 0 {
        if cycle.external_incoming.is_empty() {
            return Ok(true);
        }
        for edge in &cycle.external_incoming {
            if edge_state(edge, states, spec, 0, Some(cycle))? == EdgeState::Active {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    for edge in &cycle.carry {
        let plan_edge = PlanEdge {
            from: edge.from.clone(),
            scope: EdgeScope::Carry,
            origin: edge.origin.clone(),
        };
        if edge_state(&plan_edge, states, spec, round, Some(cycle))? == EdgeState::Active {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Build the `edges` payload for an expression join.
#[must_use]
pub fn expression_edges(gateway: &GatewaySpec, edges: &[(PlanEdge, EdgeState)]) -> Json {
    let mut values = Map::new();
    for (edge, state) in edges {
        let label = gateway
            .labels
            .iter()
            .find_map(|(label, target)| (target == &edge.from).then_some(label.as_str()))
            .unwrap_or(edge.from.as_str());
        values.insert(label.to_owned(), Json::Bool(state.is_active()));
    }
    Json::Object(values)
}

fn edge_state(
    edge: &PlanEdge,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
    round: u32,
    cycle: Option<&CycleInfo>,
) -> Result<EdgeState, ActivationError> {
    if edge.scope == EdgeScope::Carry {
        if round == 0 {
            return Ok(EdgeState::Absent);
        }
        let Some(record) = states.get(&edge.from) else {
            return Err(ActivationError::InvalidState(format!(
                "missing carry predecessor record for `{}` (needed by `{}`)",
                edge.from,
                edge_label(edge)
            )));
        };
        if record.round != round.saturating_sub(1) {
            return Err(ActivationError::InvalidState(format!(
                "carry predecessor `{}` is at round {}, expected {}",
                edge.from,
                record.round,
                round.saturating_sub(1)
            )));
        }
        return record_state(edge, record, spec);
    }

    let Some(record) = states.get(&edge.from) else {
        return Err(ActivationError::InvalidState(format!(
            "missing predecessor record for `{}` (needed by `{}`)",
            edge.from,
            edge_label(edge)
        )));
    };
    let internal =
        cycle.is_some_and(|cycle| cycle.members.iter().any(|member| member == &edge.from));
    if internal && record.round != round {
        return Err(ActivationError::InvalidState(format!(
            "predecessor `{}` is at round {}, expected {}",
            edge.from, record.round, round
        )));
    }
    record_state(edge, record, spec)
}

fn record_state(
    edge: &PlanEdge,
    record: &StepRecord,
    spec: &WorkflowSpec,
) -> Result<EdgeState, ActivationError> {
    match record.status {
        StepStatus::Succeeded => match &edge.origin {
            EdgeOrigin::Dependency => {
                if spec
                    .steps
                    .get(&edge.from)
                    .and_then(|step| step.gateway.as_ref())
                    .is_some_and(GatewaySpec::is_split)
                {
                    return Err(ActivationError::InvalidState(format!(
                        "step `{}` depends unconditionally on split gateway `{}`; use an explicit branch target",
                        edge_label(edge),
                        edge.from
                    )));
                }
                Ok(EdgeState::Active)
            }
            EdgeOrigin::Branch { branch, .. } => {
                let chosen = record
                    .output
                    .as_ref()
                    .and_then(|output| output.get("chosen"))
                    .and_then(Json::as_array)
                    .ok_or_else(|| {
                        ActivationError::InvalidState(format!(
                            "split gateway `{}` has no persisted `chosen` decision",
                            edge.from
                        ))
                    })?;
                let selected = chosen
                    .iter()
                    .any(|value| value.as_str() == Some(branch.as_str()));
                Ok(if selected {
                    EdgeState::Active
                } else {
                    EdgeState::Dead
                })
            }
        },
        StepStatus::Skipped => Ok(EdgeState::Dead),
        StepStatus::Failed => {
            let continue_on_error = spec
                .steps
                .get(&edge.from)
                .is_some_and(|step| step.continue_on_error);
            if continue_on_error {
                Ok(EdgeState::Active)
            } else {
                Err(ActivationError::InvalidState(format!(
                    "predecessor `{}` failed without continueOnError; the run should already have aborted",
                    edge.from
                )))
            }
        }
        StepStatus::Pending | StepStatus::Running => Err(ActivationError::InvalidState(format!(
            "predecessor `{}` is still {} at a group boundary",
            edge.from,
            record.status.as_str()
        ))),
    }
}

fn edge_label(edge: &PlanEdge) -> String {
    match &edge.origin {
        EdgeOrigin::Dependency => edge.from.clone(),
        EdgeOrigin::Branch { gateway, branch } => format!("{gateway}:{branch}"),
    }
}

/// Evaluate a split gateway against the current condition context.
///
/// # Errors
/// Returns [`ActivationError::Gateway`] for condition failures, no selected
/// branch, or ambiguous XOR routing. The gateway has no side effects, so the
/// engine may safely retry this after a crash.
pub fn evaluate_split(node: &PlanNode, context: &Json) -> Result<SplitDecision, ActivationError> {
    let Some(gateway) = &node.gateway else {
        return Err(ActivationError::InvalidState(format!(
            "node `{}` has no gateway payload",
            node.step
        )));
    };
    let Some(mode) = gateway.split else {
        return Err(ActivationError::InvalidState(format!(
            "gateway `{}` is not a split",
            node.step
        )));
    };

    let mut evaluations = BTreeMap::new();
    let mut true_branches = Vec::new();
    for (branch, branch_spec) in &gateway.branches {
        let is_default = gateway.default.as_deref() == Some(branch.as_str());
        let result = match &branch_spec.condition {
            None => true,
            Some(condition) => eval_condition(condition, context).map_err(|err| {
                gateway_condition_error(&node.step, &format!("branch `{branch}`: {err}"))
            })?,
        };
        evaluations.insert(
            branch.clone(),
            BranchEvaluation {
                condition: branch_spec.condition.clone(),
                result,
                default: is_default,
            },
        );
        if !is_default && result {
            true_branches.push(branch.clone());
        }
    }

    let (chosen, used_default) = match mode {
        SplitMode::And => (gateway.branches.keys().cloned().collect(), false),
        SplitMode::Or => {
            if !true_branches.is_empty() {
                (true_branches, false)
            } else if let Some(default) = &gateway.default {
                (vec![default.clone()], true)
            } else {
                return Err(gateway_error(
                    &node.step,
                    DomainError::GatewayNoBranch(node.step.clone()),
                ));
            }
        }
        SplitMode::Xor => match true_branches.len() {
            0 => match &gateway.default {
                Some(default) => (vec![default.clone()], true),
                None => {
                    return Err(gateway_error(
                        &node.step,
                        DomainError::GatewayNoBranch(node.step.clone()),
                    ));
                }
            },
            1 => (true_branches, false),
            _ => {
                return Err(gateway_error(
                    &node.step,
                    DomainError::GatewayAmbiguous(node.step.clone()),
                ));
            }
        },
    };

    Ok(SplitDecision {
        chosen,
        evaluations,
        used_default,
    })
}

fn gateway_error(gateway: &str, error: DomainError) -> ActivationError {
    ActivationError::Gateway {
        gateway: gateway.to_owned(),
        error,
    }
}

fn gateway_condition_error(gateway: &str, message: &str) -> ActivationError {
    gateway_error(
        gateway,
        DomainError::GatewayCondition {
            gateway: gateway.to_owned(),
            message: message.to_owned(),
        },
    )
}

/// Serialise a split decision into the gateway step's persisted output.
#[must_use]
pub fn split_output(gateway: &GatewaySpec, decision: &SplitDecision, round: u32) -> Json {
    let mut evaluated = Map::new();
    for (branch, evaluation) in &decision.evaluations {
        let mut entry = Map::new();
        if let Some(condition) = &evaluation.condition {
            entry.insert("condition".to_owned(), Json::String(condition.clone()));
        }
        if evaluation.default {
            entry.insert("default".to_owned(), Json::Bool(true));
        }
        entry.insert("result".to_owned(), Json::Bool(evaluation.result));
        evaluated.insert(branch.clone(), Json::Object(entry));
    }
    json!({
        "kind": "gateway",
        "mode": "split",
        "split": gateway.split.map(SplitMode::as_str),
        "evaluated": evaluated,
        "chosen": decision.chosen,
        "default": gateway.default,
        "usedDefault": decision.used_default,
        "round": round,
    })
}

/// Serialise a join decision into the join gateway step's persisted output.
#[must_use]
pub fn join_output(
    gateway: &GatewaySpec,
    incoming: &[String],
    active: &[String],
    dead: &[String],
    decision: &str,
    round: u32,
    edges: Option<&Json>,
) -> Json {
    let mut output = Map::new();
    output.insert("kind".to_owned(), Json::String("gateway".to_owned()));
    output.insert("mode".to_owned(), Json::String("join".to_owned()));
    output.insert(
        "join".to_owned(),
        gateway
            .join
            .map_or(Json::Null, |mode| Json::String(mode.as_str().to_owned())),
    );
    output.insert("incoming".to_owned(), json!(incoming));
    output.insert("active".to_owned(), json!(active));
    output.insert("dead".to_owned(), json!(dead));
    output.insert("decision".to_owned(), Json::String(decision.to_owned()));
    output.insert("round".to_owned(), Json::from(round));
    if let Some(n) = gateway.n {
        output.insert("n".to_owned(), Json::from(n));
        output.insert(
            "quorum".to_owned(),
            json!({
                "required": n,
                "active": active.len(),
                "met": active.len() >= n as usize,
            }),
        );
    }
    if let Some(expression) = &gateway.expression {
        output.insert("expression".to_owned(), Json::String(expression.clone()));
        if !gateway.labels.is_empty() {
            output.insert("labels".to_owned(), json!(gateway.labels));
        }
        if let Some(edges) = edges {
            output.insert("edges".to_owned(), edges.clone());
        }
    }
    Json::Object(output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;
    use ostinato_domain::{
        ActivationScope, BranchSpec, CarryEdge, CycleInfo, GatewaySpec, JoinMode, PlanEdge,
        SplitMode, StepSpec,
    };

    fn split_node(branches: &[(&str, Option<&str>)], default: Option<&str>) -> PlanNode {
        let mut gateway = GatewaySpec {
            split: Some(SplitMode::Xor),
            join: None,
            branches: BTreeMap::new(),
            default: default.map(ToOwned::to_owned),
            ..GatewaySpec::default()
        };
        for (name, condition) in branches {
            gateway.branches.insert(
                (*name).to_owned(),
                BranchSpec {
                    condition: condition.map(ToOwned::to_owned),
                },
            );
        }
        PlanNode {
            index: 0,
            group: 0,
            step: "route".to_owned(),
            scc: 0,
            round: 0,
            is_round_end: true,
            cycle_until: None,
            cycle_max_rounds: 1,
            in_cycle: false,
            activation: ActivationScope::Global,
            incoming: Vec::new(),
            gateway: Some(gateway),
        }
    }

    fn join_node(mode: JoinMode, sources: &[&str]) -> PlanNode {
        PlanNode {
            index: 1,
            group: 1,
            step: "merge".to_owned(),
            scc: 1,
            round: 0,
            is_round_end: true,
            cycle_until: None,
            cycle_max_rounds: 1,
            in_cycle: false,
            activation: ActivationScope::Global,
            incoming: sources
                .iter()
                .map(|source| PlanEdge {
                    from: (*source).to_owned(),
                    scope: EdgeScope::SameRound,
                    origin: EdgeOrigin::Dependency,
                })
                .collect(),
            gateway: Some(GatewaySpec {
                split: None,
                join: Some(mode),
                ..GatewaySpec::default()
            }),
        }
    }

    fn normal_node(sources: &[&str]) -> PlanNode {
        PlanNode {
            index: 2,
            group: 2,
            step: "work".to_owned(),
            scc: 2,
            round: 0,
            is_round_end: true,
            cycle_until: None,
            cycle_max_rounds: 1,
            in_cycle: false,
            activation: ActivationScope::Global,
            incoming: sources
                .iter()
                .map(|source| PlanEdge {
                    from: (*source).to_owned(),
                    scope: EdgeScope::SameRound,
                    origin: EdgeOrigin::Dependency,
                })
                .collect(),
            gateway: None,
        }
    }

    fn spec(sources: &[&str]) -> WorkflowSpec {
        let mut steps = BTreeMap::new();
        for source in sources {
            steps.insert((*source).to_owned(), StepSpec::new(*source));
        }
        steps.insert("route".to_owned(), StepSpec::new("route"));
        steps.insert("merge".to_owned(), StepSpec::new("merge"));
        steps.insert("work".to_owned(), StepSpec::new("work"));
        WorkflowSpec {
            name: "test".to_owned(),
            max_parallel: 1,
            steps,
        }
    }

    fn record(name: &str, status: StepStatus, output: Option<Json>) -> StepRecord {
        StepRecord {
            run_id: "run".to_owned(),
            name: name.to_owned(),
            status,
            visits: 1,
            attempts: 0,
            output,
            error: None,
            updated_at: "now".to_owned(),
            round: 0,
        }
    }

    fn states(entries: &[(&str, StepStatus)]) -> BTreeMap<String, StepRecord> {
        entries
            .iter()
            .map(|(name, status)| ((*name).to_owned(), record(name, *status, None)))
            .collect()
    }

    fn context(a: bool, b: bool) -> Json {
        serde_json::json!({"params": {"a": a, "b": b}})
    }

    #[test]
    fn xor_unique_true_selects_one() {
        let node = split_node(
            &[
                ("a", Some("params.a == true")),
                ("b", Some("params.b == true")),
            ],
            None,
        );
        let decision = evaluate_split(&node, &context(true, false)).expect("decision");
        assert_eq!(decision.chosen, vec!["a"]);
        assert!(!decision.used_default);
    }

    #[test]
    fn xor_no_true_uses_default() {
        let node = split_node(&[("a", Some("params.a == true")), ("b", None)], Some("b"));
        let decision = evaluate_split(&node, &context(false, false)).expect("decision");
        assert_eq!(decision.chosen, vec!["b"]);
        assert!(decision.used_default);
    }

    #[test]
    fn xor_no_true_no_default_fails() {
        let node = split_node(&[("a", Some("params.a == true"))], None);
        let error = evaluate_split(&node, &context(false, false)).expect_err("must fail");
        assert!(matches!(error, ActivationError::Gateway { .. }));
    }

    #[test]
    fn xor_two_true_is_ambiguous() {
        let node = split_node(
            &[
                ("a", Some("params.a == true")),
                ("b", Some("params.b == true")),
            ],
            None,
        );
        let error = evaluate_split(&node, &context(true, true)).expect_err("must fail");
        assert!(matches!(error, ActivationError::Gateway { .. }));
    }

    #[test]
    fn or_selects_every_true_branch() {
        let mut node = split_node(
            &[
                ("a", Some("params.a == true")),
                ("b", Some("params.b == true")),
            ],
            None,
        );
        if let Some(gateway) = &mut node.gateway {
            gateway.split = Some(SplitMode::Or);
        }
        let decision = evaluate_split(&node, &context(true, true)).expect("decision");
        assert_eq!(decision.chosen, vec!["a", "b"]);
    }

    #[test]
    fn or_none_uses_default() {
        let mut node = split_node(&[("a", Some("params.a == true")), ("b", None)], Some("b"));
        if let Some(gateway) = &mut node.gateway {
            gateway.split = Some(SplitMode::Or);
        }
        let decision = evaluate_split(&node, &context(false, false)).expect("decision");
        assert_eq!(decision.chosen, vec!["b"]);
        assert!(decision.used_default);
    }

    #[test]
    fn and_split_selects_all_branches() {
        let mut node = split_node(&[("a", None), ("b", None)], None);
        if let Some(gateway) = &mut node.gateway {
            gateway.split = Some(SplitMode::And);
        }
        let decision = evaluate_split(&node, &context(false, false)).expect("decision");
        assert_eq!(decision.chosen, vec!["a", "b"]);
    }

    #[test]
    fn and_join_requires_every_active_input() {
        let node = join_node(JoinMode::And, &["a", "b"]);
        let all = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Succeeded)]);
        assert_eq!(
            decide(&node, &all, &spec(&["a", "b"])).expect("decision"),
            Activation::Active
        );

        let dead = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Skipped)]);
        assert!(matches!(
            decide(&node, &dead, &spec(&["a", "b"])).expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn or_join_needs_one_active_input() {
        let node = join_node(JoinMode::Or, &["a", "b"]);
        let mixed = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Skipped)]);
        assert_eq!(
            decide(&node, &mixed, &spec(&["a", "b"])).expect("decision"),
            Activation::Active
        );
    }

    #[test]
    fn xor_join_two_active_is_ambiguous() {
        let node = join_node(JoinMode::Xor, &["a", "b"]);
        let both = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Succeeded)]);
        assert!(matches!(
            decide(&node, &both, &spec(&["a", "b"])).expect("decision"),
            Activation::Fail { .. }
        ));
    }

    #[test]
    fn xor_join_zero_active_is_dead() {
        let node = join_node(JoinMode::Xor, &["a", "b"]);
        let none = states(&[("a", StepStatus::Skipped), ("b", StepStatus::Skipped)]);
        assert!(matches!(
            decide(&node, &none, &spec(&["a", "b"])).expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn normal_step_with_dead_input_is_dead() {
        let node = normal_node(&["a"]);
        let dead = states(&[("a", StepStatus::Skipped)]);
        assert!(matches!(
            decide(&node, &dead, &spec(&["a"])).expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn legacy_multiple_needs_keep_and_semantics() {
        let node = normal_node(&["a", "b"]);
        let all = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Succeeded)]);
        assert_eq!(
            decide(&node, &all, &spec(&["a", "b"])).expect("decision"),
            Activation::Active
        );
    }

    #[test]
    fn missing_predecessor_is_invalid_state() {
        let node = normal_node(&["a"]);
        let empty = BTreeMap::new();
        let error = decide(&node, &empty, &spec(&["a"])).expect_err("must fail");
        assert!(matches!(error, ActivationError::InvalidState(_)));
    }

    #[test]
    fn split_gateway_branch_choice_controls_edge_state() {
        let mut node = normal_node(&[]);
        node.incoming = vec![PlanEdge {
            from: "route".to_owned(),
            scope: EdgeScope::SameRound,
            origin: EdgeOrigin::Branch {
                gateway: "route".to_owned(),
                branch: "fast".to_owned(),
            },
        }];
        let gateway = record(
            "route",
            StepStatus::Succeeded,
            Some(serde_json::json!({"chosen": ["fast"]})),
        );
        let mut map = BTreeMap::new();
        map.insert("route".to_owned(), gateway);
        assert_eq!(
            decide(&node, &map, &spec(&[])).expect("decision"),
            Activation::Active
        );

        map.get_mut("route").expect("route").output = Some(serde_json::json!({"chosen": ["slow"]}));
        assert!(matches!(
            decide(&node, &map, &spec(&[])).expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn or_no_true_no_default_fails() {
        let mut node = split_node(&[("a", Some("params.a == true"))], None);
        if let Some(gateway) = &mut node.gateway {
            gateway.split = Some(SplitMode::Or);
        }
        let error = evaluate_split(&node, &context(false, false)).expect_err("must fail");
        assert!(matches!(error, ActivationError::Gateway { .. }));
    }

    #[test]
    fn condition_evaluation_error_is_a_gateway_error() {
        let node = split_node(&[("a", Some("steps.missing.value == 1"))], None);
        let error = evaluate_split(&node, &context(true, true)).expect_err("must fail");
        assert!(matches!(error, ActivationError::Gateway { .. }));
    }

    #[test]
    fn xor_join_one_active_is_active() {
        let node = join_node(JoinMode::Xor, &["a", "b"]);
        let one = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Skipped)]);
        assert_eq!(
            decide(&node, &one, &spec(&["a", "b"])).expect("decision"),
            Activation::Active
        );
    }

    #[test]
    fn n_of_requires_quorum_and_fails_between_zero_and_n() {
        let mut node = join_node(JoinMode::NOf, &["a", "b", "c"]);
        if let Some(gateway) = &mut node.gateway {
            gateway.n = Some(2);
        }
        let all = states(&[
            ("a", StepStatus::Succeeded),
            ("b", StepStatus::Succeeded),
            ("c", StepStatus::Skipped),
        ]);
        assert_eq!(
            decide(&node, &all, &spec(&["a", "b", "c"])).expect("decision"),
            Activation::Active
        );
        let partial = states(&[
            ("a", StepStatus::Succeeded),
            ("b", StepStatus::Skipped),
            ("c", StepStatus::Skipped),
        ]);
        assert!(matches!(
            decide(&node, &partial, &spec(&["a", "b", "c"])).expect("decision"),
            Activation::Fail { .. }
        ));
        let none = states(&[
            ("a", StepStatus::Skipped),
            ("b", StepStatus::Skipped),
            ("c", StepStatus::Skipped),
        ]);
        assert!(matches!(
            decide(&node, &none, &spec(&["a", "b", "c"])).expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn expression_join_evaluates_edges() {
        let mut node = join_node(JoinMode::Expression, &["a", "b"]);
        if let Some(gateway) = &mut node.gateway {
            gateway.expression = Some("edges.a && !edges.b".to_owned());
        }
        let active = states(&[("a", StepStatus::Succeeded), ("b", StepStatus::Skipped)]);
        assert_eq!(
            decide_with_context(&node, &active, &spec(&["a", "b"]), &context(false, false))
                .expect("decision"),
            Activation::Active
        );
        let dead = states(&[("a", StepStatus::Skipped), ("b", StepStatus::Skipped)]);
        assert!(matches!(
            decide_with_context(&node, &dead, &spec(&["a", "b"]), &context(false, false))
                .expect("decision"),
            Activation::Dead { .. }
        ));
    }

    fn cycle_info() -> CycleInfo {
        CycleInfo {
            members: vec!["loop".to_owned(), "tail".to_owned()],
            entries: vec!["loop".to_owned()],
            external_incoming: Vec::new(),
            carry: vec![CarryEdge {
                from: "tail".to_owned(),
                to: "loop".to_owned(),
                origin: EdgeOrigin::Dependency,
            }],
        }
    }

    #[test]
    fn round_zero_carry_is_absent_and_root_entry_starts() {
        let mut node = normal_node(&[]);
        node.step = "loop".to_owned();
        node.activation = ActivationScope::RoundCycle;
        node.incoming = vec![PlanEdge {
            from: "tail".to_owned(),
            scope: EdgeScope::Carry,
            origin: EdgeOrigin::Dependency,
        }];
        let info = cycle_info();
        assert_eq!(
            decide_round(
                &node,
                &BTreeMap::new(),
                &spec(&[]),
                &context(false, false),
                &info,
                true
            )
            .expect("decision"),
            Activation::Active
        );
        assert!(matches!(
            decide_round(
                &node,
                &BTreeMap::new(),
                &spec(&[]),
                &context(false, false),
                &info,
                false
            )
            .expect("decision"),
            Activation::Dead { .. }
        ));
    }

    #[test]
    fn round_one_carry_reads_previous_round_and_rejects_stale_rows() {
        let mut node = normal_node(&[]);
        node.step = "loop".to_owned();
        node.activation = ActivationScope::RoundCycle;
        node.round = 1;
        node.incoming = vec![PlanEdge {
            from: "tail".to_owned(),
            scope: EdgeScope::Carry,
            origin: EdgeOrigin::Dependency,
        }];
        let info = cycle_info();
        let mut record = record("tail", StepStatus::Succeeded, None);
        record.round = 0;
        let mut map = BTreeMap::new();
        map.insert("tail".to_owned(), record);
        assert_eq!(
            decide_round(
                &node,
                &map,
                &spec(&["tail"]),
                &context(false, false),
                &info,
                true
            )
            .expect("decision"),
            Activation::Active
        );
        map.get_mut("tail").expect("tail").round = 1;
        assert!(matches!(
            decide_round(
                &node,
                &map,
                &spec(&["tail"]),
                &context(false, false),
                &info,
                true
            ),
            Err(ActivationError::InvalidState(_))
        ));
    }
}
