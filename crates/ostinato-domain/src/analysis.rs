//! Pure structural branch analysis.
//!
//! Analysis is deliberately non-blocking: it returns findings that the CLI can
//! print, and the caller decides whether warnings are fatal (`--strict`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::plan::{EdgeScope, Plan};
use crate::workflow::{JoinMode, SplitMode, WorkflowSpec};

const MAX_ANALYSIS_NODES: usize = 4096;
const MAX_FINDINGS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Note,
    Warning,
    Error,
}

impl Severity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BranchReport {
    pub findings: Vec<Finding>,
    pub truncated: bool,
}

impl BranchReport {
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity >= Severity::Warning)
    }
}

/// Analyse a materialised, acyclic plan. Cycles are skipped because their
/// rounds are not a DAG; a later release can analyse per-round scopes.
#[must_use]
pub fn analyze(spec: &WorkflowSpec, plan: &Plan) -> BranchReport {
    if plan.has_cycles || plan.nodes.len() > MAX_ANALYSIS_NODES {
        return BranchReport {
            findings: vec![Finding {
                code: "AnalysisSkipped",
                severity: Severity::Note,
                path: "workflow".to_owned(),
                message: if plan.has_cycles {
                    "static branch analysis is skipped for workflows with cycles".to_owned()
                } else {
                    format!(
                        "static branch analysis is skipped above {MAX_ANALYSIS_NODES} plan nodes"
                    )
                },
            }],
            truncated: false,
        };
    }

    let mut findings = Vec::new();
    let mut by_step: BTreeMap<&str, &crate::plan::PlanNode> = BTreeMap::new();
    for node in &plan.nodes {
        by_step.insert(node.step.as_str(), node);
    }

    let mut successors: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for node in &plan.nodes {
        for edge in &node.incoming {
            if edge.scope == EdgeScope::SameRound {
                successors
                    .entry(edge.from.as_str())
                    .or_default()
                    .insert(node.step.as_str());
            }
        }
    }

    check_shared_branch_targets(spec, &by_step, &mut findings);
    check_defaults(spec, &mut findings);

    let mut exclusive: BTreeMap<&str, BTreeMap<&str, BTreeSet<&str>>> = BTreeMap::new();
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split != Some(SplitMode::Xor) {
            continue;
        }
        let mut branch_sets: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for branch in gateway.branches.keys() {
            let mut reachable = BTreeSet::new();
            let mut queue = VecDeque::from([branch.as_str()]);
            while let Some(current) = queue.pop_front() {
                if !reachable.insert(current) {
                    continue;
                }
                if let Some(next) = successors.get(current) {
                    for target in next {
                        if !reachable.contains(target) {
                            queue.push_back(target);
                        }
                    }
                }
            }
            branch_sets.insert(branch.as_str(), reachable);
        }
        let mut exclusive_branches = BTreeMap::new();
        for (branch, reachable) in &branch_sets {
            let mut set = reachable.clone();
            for (other, other_set) in &branch_sets {
                if other != branch {
                    set.retain(|step| !other_set.contains(step));
                }
            }
            exclusive_branches.insert(*branch, set);
        }
        exclusive.insert(name.as_str(), exclusive_branches);
    }

    check_joins(spec, &by_step, &exclusive, &mut findings);
    check_branches(spec, &successors, &mut findings);

    findings.sort_by(|a, b| (&a.path, a.code).cmp(&(&b.path, b.code)));
    findings.dedup_by(|a, b| a.path == b.path && a.code == b.code);
    let truncated = findings.len() > MAX_FINDINGS;
    findings.truncate(MAX_FINDINGS);
    BranchReport {
        findings,
        truncated,
    }
}

fn check_shared_branch_targets(
    spec: &WorkflowSpec,
    by_step: &BTreeMap<&str, &crate::plan::PlanNode>,
    findings: &mut Vec<Finding>,
) {
    let mut parents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split.is_none() {
            continue;
        }
        for target in gateway.branches.keys() {
            parents
                .entry(target.as_str())
                .or_default()
                .insert(name.as_str());
        }
    }
    for (target, gateways) in parents {
        if gateways.len() > 1 {
            let path = format!("workflow.steps.{target}");
            findings.push(Finding {
                code: "GatewaySharedBranchTarget",
                severity: Severity::Warning,
                path,
                message: format!(
                    "step `{target}` is a branch target of multiple split gateways ({}); use an explicit join gateway",
                    gateways.into_iter().collect::<Vec<_>>().join(", ")
                ),
            });
        }
        if let Some(node) = by_step.get(target)
            && node
                .gateway
                .as_ref()
                .is_some_and(|gateway| gateway.split.is_some())
        {
            let path = format!("workflow.steps.{target}");
            findings.push(Finding {
                code: "GatewayBranchTargetSplit",
                severity: Severity::Note,
                path,
                message: format!(
                    "step `{target}` is a split gateway used as a branch target; confirm nested routing is intended"
                ),
            });
        }
    }
}

fn check_defaults(spec: &WorkflowSpec, findings: &mut Vec<Finding>) {
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        let Some(mode) = gateway.split else {
            continue;
        };
        if mode == SplitMode::And || gateway.default.is_some() {
            continue;
        }
        let has_unconditional = gateway.branches.values().any(|branch| {
            branch
                .condition
                .as_deref()
                .is_none_or(|condition| condition.trim() == "true")
        });
        if !has_unconditional {
            findings.push(Finding {
                code: "GatewayNoDefaultFallback",
                severity: Severity::Note,
                path: format!("workflow.steps.{name}.gateway"),
                message: format!(
                    "split gateway `{name}` has no default and no unconditional branch; `no_branch` is reachable at run time"
                ),
            });
        }
    }
}

fn check_joins(
    spec: &WorkflowSpec,
    by_step: &BTreeMap<&str, &crate::plan::PlanNode>,
    exclusive: &BTreeMap<&str, BTreeMap<&str, BTreeSet<&str>>>,
    findings: &mut Vec<Finding>,
) {
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        let Some(mode) = gateway.join else {
            continue;
        };
        let Some(node) = by_step.get(name.as_str()) else {
            continue;
        };
        let sources: Vec<&str> = node
            .incoming
            .iter()
            .filter(|edge| edge.scope == EdgeScope::SameRound)
            .map(|edge| edge.from.as_str())
            .collect();
        if sources.is_empty() {
            continue;
        }

        if mode == JoinMode::And {
            let unreachable = exclusive.values().any(|branches| {
                let occupied: BTreeSet<&str> = sources
                    .iter()
                    .filter_map(|source| {
                        branches
                            .iter()
                            .find_map(|(branch, set)| set.contains(source).then_some(*branch))
                    })
                    .collect();
                occupied.len() > 1
            });
            if unreachable {
                findings.push(Finding {
                    code: "GatewayJoinUnreachable",
                    severity: Severity::Warning,
                    path: format!("workflow.steps.{name}.gateway"),
                    message: format!(
                        "AND join `{name}` has inputs in mutually exclusive branches of an XOR split"
                    ),
                });
            }
        }

        if mode == JoinMode::Xor && sources.len() > 1 {
            let max_active = max_active_inputs(&sources, exclusive);
            if max_active > 1 {
                findings.push(Finding {
                    code: "GatewayXorJoinAmbiguityPotential",
                    severity: Severity::Warning,
                    path: format!("workflow.steps.{name}.gateway"),
                    message: format!(
                        "XOR join `{name}` has inputs that can be active together (up to {max_active})"
                    ),
                });
            }
        }

        if mode == JoinMode::NOf
            && let Some(required) = gateway.n
        {
            let max_active = max_active_inputs(&sources, exclusive);
            if required as usize > max_active {
                findings.push(Finding {
                    code: "GatewayQuorumUnreachable",
                    severity: Severity::Warning,
                    path: format!("workflow.steps.{name}.gateway.n"),
                    message: format!(
                        "quorum `{required}` exceeds the maximum simultaneously active inputs ({max_active})"
                    ),
                });
            }
        }
    }
}

fn max_active_inputs(
    sources: &[&str],
    exclusive: &BTreeMap<&str, BTreeMap<&str, BTreeSet<&str>>>,
) -> usize {
    // A sound maximum independent set over the "cannot both be active" graph.
    // The plan caps analysis to small graphs; fall back to the trivial bound
    // above 20 inputs rather than risk an exponential blow-up.
    if sources.len() > 20 {
        return sources.len();
    }
    let mut conflict = vec![vec![false; sources.len()]; sources.len()];
    for branches in exclusive.values() {
        let branch_of: Vec<Option<&str>> = sources
            .iter()
            .map(|source| {
                branches
                    .iter()
                    .find_map(|(branch, set)| set.contains(source).then_some(*branch))
            })
            .collect();
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                if let (Some(left), Some(right)) = (branch_of[i], branch_of[j])
                    && left != right
                {
                    conflict[i][j] = true;
                    conflict[j][i] = true;
                }
            }
        }
    }
    fn search(index: usize, chosen: &mut Vec<usize>, conflict: &[Vec<bool>]) -> usize {
        if index == conflict.len() {
            return chosen.len();
        }
        let mut best = search(index + 1, chosen, conflict);
        if chosen.iter().all(|&other| !conflict[index][other]) {
            chosen.push(index);
            best = best.max(search(index + 1, chosen, conflict));
            chosen.pop();
        }
        best
    }
    search(0, &mut Vec::new(), &conflict)
}

fn check_branches(
    spec: &WorkflowSpec,
    successors: &BTreeMap<&str, BTreeSet<&str>>,
    findings: &mut Vec<Finding>,
) {
    for (name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split.is_none() {
            continue;
        }
        let mut common_joins: Option<BTreeSet<&str>> = None;
        for branch in gateway.branches.keys() {
            let mut reachable = BTreeSet::new();
            let mut queue = VecDeque::from([branch.as_str()]);
            while let Some(current) = queue.pop_front() {
                if !reachable.insert(current) {
                    continue;
                }
                if let Some(next) = successors.get(current) {
                    for target in next {
                        if !reachable.contains(target) {
                            queue.push_back(target);
                        }
                    }
                }
            }
            let joins: BTreeSet<&str> = reachable
                .iter()
                .copied()
                .filter(|step| {
                    spec.steps
                        .get(*step)
                        .and_then(|spec| spec.gateway.as_ref())
                        .is_some_and(|gateway| gateway.join.is_some())
                })
                .collect();
            common_joins = Some(match common_joins {
                None => joins,
                Some(current) => current.intersection(&joins).copied().collect(),
            });
            let has_work = reachable.iter().any(|step| {
                spec.steps
                    .get(*step)
                    .is_some_and(|spec| spec.kind != crate::workflow::StepKind::Gateway)
            });
            if !has_work {
                findings.push(Finding {
                    code: "GatewayBranchNoEffect",
                    severity: Severity::Warning,
                    path: format!("workflow.steps.{name}.gateway.branches.{branch}"),
                    message: format!("branch `{branch}` of `{name}` reaches no non-gateway step"),
                });
            }
        }
        if common_joins.is_some_and(|joins| joins.is_empty()) {
            findings.push(Finding {
                code: "GatewayBranchEscapesJoin",
                severity: Severity::Warning,
                path: format!("workflow.steps.{name}.gateway"),
                message: format!(
                    "branches of `{name}` do not share a common post-dominator join gateway"
                ),
            });
        }
    }
}
