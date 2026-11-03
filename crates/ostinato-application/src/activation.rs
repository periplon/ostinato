//! Pure gateway activation logic.
//!
//! This module contains no I/O and depends only on the domain model and the
//! pure condition interpreter. The engine persists the decisions it produces.

use std::collections::BTreeMap;

use ostinato_domain::{
    DomainError, EdgeOrigin, GatewaySpec, JoinMode, PlanEdge, PlanNode, SplitMode, StepRecord,
    StepStatus, WorkflowSpec,
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
    Fail { error: String },
}

/// State of one incoming edge at a group boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeState {
    Active,
    Dead,
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
    let mut edges = Vec::with_capacity(node.incoming.len());
    for edge in &node.incoming {
        edges.push((edge.clone(), edge_state(edge, states, spec)?));
    }
    Ok(edges)
}

/// Decide how a node is activated from its persisted predecessor records.
///
/// # Errors
/// Returns [`ActivationError`] when the checkpoint does not contain enough
/// information to reconstruct the edge states.
pub fn decide(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
) -> Result<Activation, ActivationError> {
    if node.in_cycle || node.incoming.is_empty() {
        // Legacy cycle semantics are untouched: cycle members always execute
        // in each materialised round. Gateways are rejected inside cycles.
        return Ok(Activation::Active);
    }

    let edges = incoming_edges(node, states, spec)?;
    let active = edges.iter().filter(|(_, state)| state.is_active()).count();
    let dead = edges.len() - active;

    if let Some(gateway) = &node.gateway
        && let Some(mode) = gateway.join
    {
        return Ok(join_activation(&node.step, mode, active, dead));
    }

    // Normal steps and split gateways both require every prerequisite edge to
    // be active. Legacy multi-`needs` steps keep their implicit AND semantics.
    if dead > 0 {
        Ok(Activation::Dead {
            reason: "dead_path".to_owned(),
        })
    } else {
        Ok(Activation::Active)
    }
}

fn join_activation(step: &str, mode: JoinMode, active: usize, dead: usize) -> Activation {
    match mode {
        JoinMode::And => {
            if dead > 0 {
                Activation::Dead {
                    reason: "dead_path".to_owned(),
                }
            } else {
                Activation::Active
            }
        }
        JoinMode::Or => {
            if active > 0 {
                Activation::Active
            } else {
                Activation::Dead {
                    reason: "dead_path".to_owned(),
                }
            }
        }
        JoinMode::Xor => match active {
            0 => Activation::Dead {
                reason: "dead_path".to_owned(),
            },
            1 => Activation::Active,
            _ => Activation::Fail {
                error: format!(
                    "XOR join `{step}` has {active} active incoming branches; exactly one is allowed"
                ),
            },
        },
    }
}

fn edge_state(
    edge: &PlanEdge,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
) -> Result<EdgeState, ActivationError> {
    let Some(record) = states.get(&edge.from) else {
        return Err(ActivationError::InvalidState(format!(
            "missing predecessor record for `{}` (needed by `{}`)",
            edge.from,
            edge_label(edge)
        )));
    };
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
) -> Json {
    json!({
        "kind": "gateway",
        "mode": "join",
        "join": gateway.join.map(JoinMode::as_str),
        "incoming": incoming,
        "active": active,
        "dead": dead,
        "decision": decision,
        "round": round,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;
    use ostinato_domain::{BranchSpec, GatewaySpec, JoinMode, SplitMode, StepSpec};

    fn split_node(branches: &[(&str, Option<&str>)], default: Option<&str>) -> PlanNode {
        let mut gateway = GatewaySpec {
            split: Some(SplitMode::Xor),
            join: None,
            branches: BTreeMap::new(),
            default: default.map(ToOwned::to_owned),
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
            incoming: sources
                .iter()
                .map(|source| PlanEdge {
                    from: (*source).to_owned(),
                    origin: EdgeOrigin::Dependency,
                })
                .collect(),
            gateway: Some(GatewaySpec {
                split: None,
                join: Some(mode),
                branches: BTreeMap::new(),
                default: None,
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
            incoming: sources
                .iter()
                .map(|source| PlanEdge {
                    from: (*source).to_owned(),
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
            origin: EdgeOrigin::Branch {
                gateway: "route".to_owned(),
                branch: "fast".to_owned(),
            },
        }];
        let mut gateway = record(
            "route",
            StepStatus::Succeeded,
            Some(serde_json::json!({"chosen": ["fast"]})),
        );
        gateway.output = Some(serde_json::json!({"chosen": ["fast"]}));
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
}
