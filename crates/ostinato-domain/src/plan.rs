use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::workflow::{
    EventSource, GatewaySpec, RetryPolicy, StepKind, WorkflowSpec, validate_workflow_event,
};

/// Hard cap on materialised plan nodes. Cycles expand eagerly; this prevents
/// a workflow file from exhausting memory during planning.
pub const MAX_PLAN_NODES: usize = 100_000;

type CycleLayoutMap = BTreeMap<usize, (Vec<Vec<usize>>, BTreeSet<(usize, usize)>)>;

/// Where an incoming plan edge came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeOrigin {
    /// An explicit `needs` dependency.
    Dependency,
    /// An implicit edge created by a split gateway branch.
    Branch { gateway: String, branch: String },
}

/// Whether an edge is evaluated in the current cycle round or carried from the
/// previous round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeScope {
    #[default]
    SameRound,
    Carry,
}

/// One incoming edge of a plan node. Metadata only; not persisted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlanEdge {
    pub from: String,
    #[serde(default, skip_serializing_if = "is_same_round")]
    pub scope: EdgeScope,
    pub origin: EdgeOrigin,
}

fn is_same_round(scope: &EdgeScope) -> bool {
    *scope == EdgeScope::SameRound
}

/// How activation is computed for a plan node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationScope {
    /// Ordinary DAG node.
    #[default]
    Global,
    /// Member of a legacy cycle without gateways; always active.
    LegacyCycle,
    /// Member of a cycle with at least one gateway; round-scoped activation.
    RoundCycle,
}

/// A removed back-edge that carries a token into the next cycle round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarryEdge {
    pub from: String,
    pub to: String,
    pub origin: EdgeOrigin,
}

/// Stable metadata for a gateway-bearing cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleInfo {
    /// Sorted member step names.
    pub members: Vec<String>,
    /// Nodes with no internal same-round incoming edge.
    pub entries: Vec<String>,
    /// Same-round edges entering the cycle from outside.
    pub external_incoming: Vec<PlanEdge>,
    /// Removed back-edges, with both endpoints.
    pub carry: Vec<CarryEdge>,
}

/// One scheduled execution of a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanNode {
    /// Position in the plan. Persisted as `runs.resume_index`.
    pub index: usize,
    /// Consecutive nodes sharing a group may run in parallel.
    pub group: u64,
    pub step: String,
    pub scc: usize,
    pub round: u32,
    /// True for the last node of a cycle round. The engine evaluates `until`
    /// when it finishes.
    pub is_round_end: bool,
    pub cycle_until: Option<String>,
    pub cycle_max_rounds: u32,
    /// True when this node belongs to a materialised cycle.
    #[serde(default)]
    pub in_cycle: bool,
    /// How activation is derived for this node.
    #[serde(default)]
    pub activation: ActivationScope,
    /// Incoming edges in deterministic `(from, scope, origin)` order.
    #[serde(default)]
    pub incoming: Vec<PlanEdge>,
    /// Gateway routing definition, if this node is a gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewaySpec>,
}

/// A fully materialised execution plan, including every cycle round.
///
/// Materialising cycles has a deliberate advantage for durability: every
/// visit has a stable index, so a crash can resume at an exact position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub nodes: Vec<PlanNode>,
    pub groups: u64,
    pub has_cycles: bool,
    #[serde(default)]
    pub has_gateways: bool,
    #[serde(default)]
    pub has_gateway_cycles: bool,
    /// Stable cycle metadata, sorted by member names.
    #[serde(default)]
    pub cycle_info: Vec<CycleInfo>,
}

/// Build a deterministic execution plan from a workflow specification.
///
/// # Errors
/// Returns a [`DomainError`] for unknown dependencies, cycles without a
/// policy, conflicting `until` conditions, or invalid limits.
pub fn build_plan(spec: &WorkflowSpec) -> Result<Plan, DomainError> {
    if spec.steps.is_empty() {
        return Err(DomainError::EmptyWorkflow);
    }
    if spec.max_parallel == 0 {
        return Err(DomainError::InvalidParallelism);
    }

    let names: Vec<String> = spec.steps.keys().cloned().collect();
    let index_of: BTreeMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    validate_gateways(spec, &index_of)?;

    let has_gateways = spec
        .steps
        .values()
        .any(|step| step.kind == StepKind::Gateway);

    // Execution graph: dependency -> dependent.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    let mut incoming: Vec<Vec<PlanEdge>> = vec![Vec::new(); names.len()];
    let mut has_self_edge = vec![false; names.len()];
    for (dependent, name) in names.iter().enumerate() {
        let step = &spec.steps[name];
        for need in &step.needs {
            let Some(&dependency) = index_of.get(need.as_str()) else {
                return Err(DomainError::UnknownDependency(need.clone()));
            };
            if dependency == dependent {
                if step.cycle.is_none() {
                    return Err(DomainError::SelfDependency(name.clone()));
                }
                has_self_edge[dependent] = true;
            }
            adj[dependency].push(dependent);
            incoming[dependent].push(PlanEdge {
                from: need.clone(),
                scope: EdgeScope::SameRound,
                origin: EdgeOrigin::Dependency,
            });
        }
    }

    // Implicit branch edges: split gateway -> branch target.
    for (gateway_name, step) in &spec.steps {
        let Some(gateway) = &step.gateway else {
            continue;
        };
        let Some(_split) = gateway.split else {
            continue;
        };
        let Some(&gateway_index) = index_of.get(gateway_name.as_str()) else {
            continue;
        };
        for target in gateway.branches.keys() {
            let Some(&target_index) = index_of.get(target.as_str()) else {
                continue;
            };
            adj[gateway_index].push(target_index);
            incoming[target_index].push(PlanEdge {
                from: gateway_name.clone(),
                scope: EdgeScope::SameRound,
                origin: EdgeOrigin::Branch {
                    gateway: gateway_name.clone(),
                    branch: target.clone(),
                },
            });
        }
    }
    for edges in &mut adj {
        edges.sort_unstable();
        edges.dedup();
    }
    for edges in &mut incoming {
        edges.sort();
        edges.dedup();
    }

    let components = strongly_connected(&adj);
    let mut comp_of = vec![0usize; names.len()];
    for (comp_id, component) in components.iter().enumerate() {
        for &node in component {
            comp_of[node] = comp_id;
        }
    }

    let mut comp_nodes: Vec<Vec<usize>> = components.clone();
    for component in &mut comp_nodes {
        component.sort_by(|a, b| names[*a].cmp(&names[*b]));
    }

    // Materialise the cycle layout up-front: levels plus the back-edges that
    // become carry edges in the next round.
    let mut cycle_layouts: CycleLayoutMap = BTreeMap::new();
    let mut gateway_cycles: BTreeSet<usize> = BTreeSet::new();
    for (comp_id, component) in comp_nodes.iter().enumerate() {
        if !is_cyclic(comp_id, &comp_nodes, &has_self_edge, &comp_of, &adj) {
            continue;
        }
        let external_targets: BTreeSet<usize> = component
            .iter()
            .copied()
            .filter(|node| {
                incoming[*node].iter().any(|edge| {
                    edge.scope == EdgeScope::SameRound
                        && index_of
                            .get(edge.from.as_str())
                            .is_some_and(|from| comp_of[*from] != comp_id)
                })
            })
            .collect();
        let (levels, carry) = cycle_layout(component, &adj, &names, spec, &external_targets);
        if component
            .iter()
            .any(|node| spec.steps[&names[*node]].kind == StepKind::Gateway)
        {
            gateway_cycles.insert(comp_id);
        }
        cycle_layouts.insert(comp_id, (levels, carry));
    }

    let activation_for = |comp_id: usize| -> ActivationScope {
        if !is_cyclic(comp_id, &comp_nodes, &has_self_edge, &comp_of, &adj) {
            ActivationScope::Global
        } else if gateway_cycles.contains(&comp_id) {
            ActivationScope::RoundCycle
        } else {
            ActivationScope::LegacyCycle
        }
    };

    // A split gateway inside a cycle may not branch out of it; a split
    // outside a cycle may not branch into one. Both would bypass round-scoped
    // activation.
    if has_gateways {
        for (name, step) in &spec.steps {
            let Some(gateway) = &step.gateway else {
                continue;
            };
            if gateway.split.is_none() {
                continue;
            }
            let Some(&gateway_index) = index_of.get(name.as_str()) else {
                continue;
            };
            let gateway_comp = comp_of[gateway_index];
            let gateway_cyclic = cycle_layouts.contains_key(&gateway_comp);
            for target in gateway.branches.keys() {
                let Some(&target_index) = index_of.get(target.as_str()) else {
                    continue;
                };
                let target_cyclic = cycle_layouts.contains_key(&comp_of[target_index]);
                if gateway_cyclic && comp_of[target_index] != gateway_comp {
                    return Err(DomainError::GatewayBranchLeavesCycle {
                        gateway: name.clone(),
                        target: target.clone(),
                    });
                }
                if !gateway_cyclic && target_cyclic {
                    return Err(DomainError::GatewayBranchIntoCycle {
                        gateway: name.clone(),
                        target: target.clone(),
                    });
                }
            }
        }
    }

    // Mark removed back-edges as carry edges on the target's incoming list.
    for (_, carry) in cycle_layouts.values() {
        for &(from, to) in carry {
            for edge in &mut incoming[to] {
                if edge.from == names[from] {
                    edge.scope = EdgeScope::Carry;
                }
            }
        }
    }
    for edges in &mut incoming {
        edges.sort();
        edges.dedup();
    }

    if has_gateways {
        for &comp_id in cycle_layouts.keys() {
            if !gateway_cycles.contains(&comp_id) {
                continue;
            }
            for &node in &comp_nodes[comp_id] {
                let Some(gateway) = &spec.steps[&names[node]].gateway else {
                    continue;
                };
                if gateway.join.is_some()
                    && incoming[node]
                        .iter()
                        .any(|edge| edge.scope == EdgeScope::Carry)
                    && incoming[node]
                        .iter()
                        .any(|edge| edge.scope == EdgeScope::SameRound)
                {
                    return Err(DomainError::GatewayCycleMixedScopeJoin(names[node].clone()));
                }
            }
        }
        for (node, name) in names.iter().enumerate() {
            if spec.steps[name].kind == StepKind::Gateway {
                continue;
            }
            if activation_for(comp_of[node]) == ActivationScope::LegacyCycle {
                continue;
            }
            if incoming[node]
                .iter()
                .filter(|edge| edge.scope == EdgeScope::SameRound)
                .count()
                > 1
            {
                return Err(DomainError::ImplicitJoinWithGateways(name.clone()));
            }
        }
    }

    // Condensation DAG and its levels.
    let mut comp_adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); components.len()];
    let mut indegree = vec![0usize; components.len()];
    for u in 0..names.len() {
        for &v in &adj[u] {
            let (cu, cv) = (comp_of[u], comp_of[v]);
            if cu != cv && comp_adj[cu].insert(cv) {
                indegree[cv] += 1;
            }
        }
    }
    let mut queue: VecDeque<usize> = (0..components.len())
        .filter(|id| indegree[*id] == 0)
        .collect();
    let mut comp_level = vec![0usize; components.len()];
    let mut topo = Vec::with_capacity(components.len());
    while let Some(id) = queue.pop_front() {
        topo.push(id);
        let next: Vec<usize> = comp_adj[id].iter().copied().collect();
        for target in next {
            comp_level[target] = comp_level[target].max(comp_level[id] + 1);
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if topo.len() != components.len() {
        return Err(DomainError::EmptyWorkflow);
    }

    let mut by_level: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (id, level) in comp_level.iter().enumerate() {
        by_level.entry(*level).or_default().push(id);
    }

    let mut nodes = Vec::new();
    let mut group = 0u64;
    let mut has_cycles = false;

    for level in 0..=by_level.keys().copied().max().unwrap_or(0) {
        let Some(comp_ids) = by_level.get(&level) else {
            continue;
        };

        // Acyclic components at this level run together in one parallel group.
        let mut acyclic: Vec<usize> = Vec::new();
        let mut cyclic: Vec<usize> = Vec::new();
        for &comp_id in comp_ids {
            if is_cyclic(comp_id, &comp_nodes, &has_self_edge, &comp_of, &adj) {
                cyclic.push(comp_id);
            } else {
                acyclic.push(comp_id);
            }
        }
        acyclic.sort_by(|a, b| names[comp_nodes[*a][0]].cmp(&names[comp_nodes[*b][0]]));
        cyclic.sort_by(|a, b| names[comp_nodes[*a][0]].cmp(&names[comp_nodes[*b][0]]));

        if !acyclic.is_empty() {
            for &comp_id in &acyclic {
                let member = comp_nodes[comp_id][0];
                if spec.steps[&names[member]].cycle.is_some() {
                    return Err(DomainError::CycleOnAcyclicStep(names[member].clone()));
                }
                if nodes.len() >= MAX_PLAN_NODES {
                    return Err(DomainError::PlanTooLarge {
                        max: MAX_PLAN_NODES,
                    });
                }
                nodes.push(PlanNode {
                    index: nodes.len(),
                    group,
                    step: names[member].clone(),
                    scc: comp_id,
                    round: 0,
                    is_round_end: true,
                    cycle_until: None,
                    cycle_max_rounds: 1,
                    in_cycle: false,
                    activation: ActivationScope::Global,
                    incoming: incoming[member].clone(),
                    gateway: spec.steps[&names[member]].gateway.clone(),
                });
            }
            group += 1;
        }

        for comp_id in cyclic {
            has_cycles = true;
            let members = &comp_nodes[comp_id];
            let (max_rounds, until) = cycle_policy(spec, members, &names)?;
            let Some((levels, _)) = cycle_layouts.get(&comp_id) else {
                return Err(DomainError::EmptyWorkflow);
            };
            let activation = if gateway_cycles.contains(&comp_id) {
                ActivationScope::RoundCycle
            } else {
                ActivationScope::LegacyCycle
            };
            for round in 0..max_rounds {
                for (level_idx, inner) in levels.iter().enumerate() {
                    for &node in inner {
                        if nodes.len() >= MAX_PLAN_NODES {
                            return Err(DomainError::PlanTooLarge {
                                max: MAX_PLAN_NODES,
                            });
                        }
                        nodes.push(PlanNode {
                            index: nodes.len(),
                            group,
                            step: names[node].clone(),
                            scc: comp_id,
                            round,
                            is_round_end: level_idx + 1 == levels.len(),
                            cycle_until: until.clone(),
                            cycle_max_rounds: max_rounds,
                            in_cycle: true,
                            activation,
                            incoming: incoming[node].clone(),
                            gateway: spec.steps[&names[node]].gateway.clone(),
                        });
                    }
                    group += 1;
                }
            }
        }
    }

    let mut cycle_info = Vec::new();
    for &comp_id in &gateway_cycles {
        let members = comp_nodes[comp_id].clone();
        let member_names: Vec<String> = members.iter().map(|node| names[*node].clone()).collect();
        let mut external_incoming = Vec::new();
        let mut entries = Vec::new();
        for &node in &members {
            let internal_same_round = incoming[node].iter().any(|edge| {
                edge.scope == EdgeScope::SameRound
                    && index_of
                        .get(edge.from.as_str())
                        .is_some_and(|from| comp_of[*from] == comp_id)
            });
            if !internal_same_round {
                entries.push(names[node].clone());
            }
            for edge in &incoming[node] {
                if edge.scope == EdgeScope::SameRound
                    && index_of
                        .get(edge.from.as_str())
                        .is_some_and(|from| comp_of[*from] != comp_id)
                {
                    external_incoming.push(edge.clone());
                }
            }
        }
        entries.sort();
        external_incoming.sort();
        let mut carry = Vec::new();
        if let Some((_, carry_edges)) = cycle_layouts.get(&comp_id) {
            for &(from, to) in carry_edges {
                let origin = incoming[to]
                    .iter()
                    .find(|edge| edge.from == names[from])
                    .map_or(EdgeOrigin::Dependency, |edge| edge.origin.clone());
                carry.push(CarryEdge {
                    from: names[from].clone(),
                    to: names[to].clone(),
                    origin,
                });
            }
        }
        carry.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        cycle_info.push(CycleInfo {
            members: member_names,
            entries,
            external_incoming,
            carry,
        });
    }
    cycle_info.sort_by(|a, b| a.members.cmp(&b.members));

    validate_awaits(spec, &nodes)?;

    Ok(Plan {
        groups: group,
        has_cycles,
        has_gateways,
        has_gateway_cycles: !gateway_cycles.is_empty(),
        cycle_info,
        nodes,
    })
}

/// Structural gateway validation. The compiler reports the same rules with
/// full field paths; this keeps programmatic callers honest.
fn validate_gateways(
    spec: &WorkflowSpec,
    index_of: &BTreeMap<&str, usize>,
) -> Result<(), DomainError> {
    let split_gateways: BTreeSet<&str> = spec
        .steps
        .iter()
        .filter(|(_, step)| step.gateway.as_ref().is_some_and(GatewaySpec::is_split))
        .map(|(name, _)| name.as_str())
        .collect();
    for (name, step) in &spec.steps {
        for need in &step.needs {
            if split_gateways.contains(need.as_str()) {
                return Err(DomainError::GatewayNeedsRejected {
                    gateway: need.clone(),
                    step: name.clone(),
                });
            }
        }
    }

    for (name, step) in &spec.steps {
        match (step.kind, &step.gateway) {
            (StepKind::Gateway, None) => {
                return Err(DomainError::GatewayRequiresSpec(name.clone()));
            }
            (StepKind::Gateway, Some(_)) => {}
            (_, Some(_)) => {
                return Err(DomainError::GatewayOnNonGateway(name.clone()));
            }
            (_, None) => continue,
        }
        let gateway = step
            .gateway
            .as_ref()
            .ok_or_else(|| DomainError::GatewayRequiresSpec(name.clone()))?;
        for (present, field) in [
            (
                step.retry != crate::workflow::RetryPolicy::default(),
                "retry",
            ),
            (step.cycle.is_some(), "cycle"),
            (step.exec.is_some(), "exec"),
            (step.agent.is_some(), "agent"),
            (step.improve.is_some(), "improve"),
            (step.await_spec.is_some(), "await"),
            (!step.emit.is_empty(), "emit"),
            (step.timeout_ms.is_some(), "timeoutMs"),
            (step.continue_on_error, "continueOnError"),
        ] {
            if present {
                return Err(DomainError::GatewayPayloadNotAllowed {
                    step: name.clone(),
                    field: field.to_owned(),
                });
            }
        }
        match (gateway.split, gateway.join) {
            (Some(_), Some(_)) => return Err(DomainError::GatewaySplitAndJoin(name.clone())),
            (None, None) => return Err(DomainError::GatewayMissingMode(name.clone())),
            (Some(mode), None) => {
                if gateway.n.is_some() || gateway.expression.is_some() || !gateway.labels.is_empty()
                {
                    return Err(DomainError::GatewayQuorumOnWrongMode(name.clone()));
                }
                if gateway.branches.is_empty() {
                    return Err(DomainError::GatewayEmptyBranches(name.clone()));
                }
                if let Some(default) = &gateway.default
                    && !gateway.branches.contains_key(default)
                {
                    return Err(DomainError::GatewayInvalidDefault {
                        gateway: name.clone(),
                        branch: default.clone(),
                    });
                }
                for (branch, branch_spec) in &gateway.branches {
                    if !index_of.contains_key(branch.as_str()) {
                        return Err(DomainError::GatewayUnknownBranchTarget {
                            gateway: name.clone(),
                            target: branch.clone(),
                        });
                    }
                    if branch == name {
                        return Err(DomainError::GatewaySelfBranch(name.clone()));
                    }
                    let target = &spec.steps[branch.as_str()];
                    if target.kind == StepKind::Gateway
                        && target.gateway.as_ref().is_some_and(GatewaySpec::is_join)
                    {
                        return Err(DomainError::GatewayBranchTargetIsJoin {
                            gateway: name.clone(),
                            target: branch.clone(),
                        });
                    }
                    if target.needs.iter().any(|need| need == name) {
                        return Err(DomainError::GatewayNeedsRejected {
                            gateway: name.clone(),
                            step: branch.clone(),
                        });
                    }
                    if !target.needs.is_empty() {
                        return Err(DomainError::GatewayBranchTargetHasNeeds {
                            gateway: name.clone(),
                            target: branch.clone(),
                        });
                    }
                    let is_default = gateway.default.as_deref() == Some(branch.as_str());
                    match mode {
                        crate::workflow::SplitMode::And => {
                            if branch_spec.condition.is_some() {
                                return Err(DomainError::GatewayAndCondition {
                                    gateway: name.clone(),
                                    branch: branch.clone(),
                                });
                            }
                        }
                        crate::workflow::SplitMode::Or | crate::workflow::SplitMode::Xor => {
                            if is_default {
                                if branch_spec.condition.is_some() {
                                    return Err(DomainError::GatewayInvalidDefault {
                                        gateway: name.clone(),
                                        branch: branch.clone(),
                                    });
                                }
                            } else if mode == crate::workflow::SplitMode::Xor
                                && branch_spec.condition.is_none()
                            {
                                return Err(DomainError::GatewayMissingCondition {
                                    gateway: name.clone(),
                                    branch: branch.clone(),
                                });
                            }
                        }
                    }
                }
                if mode == crate::workflow::SplitMode::And && gateway.default.is_some() {
                    let branch = gateway.default.clone().unwrap_or_default();
                    return Err(DomainError::GatewayInvalidDefault {
                        gateway: name.clone(),
                        branch,
                    });
                }
            }
            (None, Some(mode)) => {
                if step.needs.is_empty() {
                    return Err(DomainError::GatewayJoinWithoutNeeds(name.clone()));
                }
                if !gateway.branches.is_empty() || gateway.default.is_some() {
                    return Err(DomainError::GatewayJoinHasBranches(name.clone()));
                }
                if gateway.split.is_none()
                    && gateway.n.is_some()
                    && mode != crate::workflow::JoinMode::NOf
                {
                    return Err(DomainError::GatewayQuorumOnWrongMode(name.clone()));
                }
                match mode {
                    crate::workflow::JoinMode::NOf => {
                        let Some(n) = gateway.n else {
                            return Err(DomainError::GatewayQuorumMissingN(name.clone()));
                        };
                        let mut distinct = BTreeSet::new();
                        for need in &step.needs {
                            if !distinct.insert(need.as_str()) {
                                return Err(DomainError::GatewayDuplicateNeeds {
                                    gateway: name.clone(),
                                    need: need.clone(),
                                });
                            }
                        }
                        if n == 0 || n as usize > distinct.len() {
                            return Err(DomainError::GatewayQuorumInvalidN {
                                gateway: name.clone(),
                                n,
                                max: distinct.len(),
                            });
                        }
                        if gateway.expression.is_some() || !gateway.labels.is_empty() {
                            return Err(DomainError::GatewayJoinHasBranches(name.clone()));
                        }
                    }
                    crate::workflow::JoinMode::Expression => {
                        if gateway
                            .expression
                            .as_deref()
                            .is_none_or(|expr| expr.trim().is_empty())
                        {
                            return Err(DomainError::GatewayExpressionMissing(name.clone()));
                        }
                        if gateway.n.is_some() {
                            return Err(DomainError::GatewayQuorumOnWrongMode(name.clone()));
                        }
                        if !gateway.labels.is_empty() {
                            let needs: BTreeSet<&str> =
                                step.needs.iter().map(String::as_str).collect();
                            let mut values = BTreeSet::new();
                            for (label, target) in &gateway.labels {
                                if !is_valid_label(label) {
                                    return Err(DomainError::GatewayInvalidLabel {
                                        gateway: name.clone(),
                                        label: label.clone(),
                                    });
                                }
                                if !needs.contains(target.as_str()) {
                                    return Err(DomainError::GatewayLabelUnknownStep {
                                        gateway: name.clone(),
                                        label: label.clone(),
                                    });
                                }
                                if !values.insert(target.as_str()) {
                                    return Err(DomainError::GatewayDuplicateLabelTarget {
                                        gateway: name.clone(),
                                        target: target.clone(),
                                    });
                                }
                            }
                            if gateway.labels.len() != needs.len() {
                                let missing = needs
                                    .iter()
                                    .find(|need| !values.contains(**need))
                                    .map(|need| (*need).to_owned())
                                    .unwrap_or_default();
                                return Err(DomainError::GatewayLabelMissing {
                                    gateway: name.clone(),
                                    target: missing,
                                });
                            }
                        }
                    }
                    _ => {
                        if gateway.n.is_some()
                            || gateway.expression.is_some()
                            || !gateway.labels.is_empty()
                        {
                            return Err(DomainError::GatewayQuorumOnWrongMode(name.clone()));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_cyclic(
    comp: usize,
    comp_nodes: &[Vec<usize>],
    has_self_edge: &[bool],
    comp_of: &[usize],
    adj: &[Vec<usize>],
) -> bool {
    if comp_nodes[comp].len() > 1 {
        return true;
    }
    let node = comp_nodes[comp][0];
    has_self_edge[node] || adj[node].iter().any(|&next| comp_of[next] == comp)
}

fn cycle_policy(
    spec: &WorkflowSpec,
    members: &[usize],
    names: &[String],
) -> Result<(u32, Option<String>), DomainError> {
    let mut max_rounds: Option<u32> = None;
    let mut until: Option<String> = None;
    for &member in members {
        let step = &spec.steps[&names[member]];
        if let Some(cycle) = &step.cycle {
            if cycle.max_rounds == 0 {
                return Err(DomainError::InvalidCycleLimit(names[member].clone()));
            }
            max_rounds =
                Some(max_rounds.map_or(cycle.max_rounds, |current| current.min(cycle.max_rounds)));
            if let Some(condition) = &cycle.until {
                if let Some(existing) = &until {
                    if existing != condition {
                        return Err(DomainError::ConflictingCycleConditions {
                            steps: members.iter().map(|i| names[*i].clone()).collect(),
                        });
                    }
                } else {
                    until = Some(condition.clone());
                }
            }
        }
    }
    let Some(max_rounds) = max_rounds else {
        return Err(DomainError::CycleWithoutPolicy {
            steps: members.iter().map(|i| names[*i].clone()).collect(),
        });
    };
    Ok((max_rounds, until))
}

fn is_valid_label(label: &str) -> bool {
    if matches!(label, "and" | "or" | "not" | "true" | "false" | "null") {
        return false;
    }
    let mut chars = label.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Remove DFS back-edges, then compute Kahn levels of the remaining DAG.
/// Returns the levels and the removed `(from, to)` back-edges.
fn cycle_layout(
    members: &[usize],
    adj: &[Vec<usize>],
    names: &[String],
    spec: &WorkflowSpec,
    external_targets: &BTreeSet<usize>,
) -> (Vec<Vec<usize>>, BTreeSet<(usize, usize)>) {
    let member_set: BTreeSet<usize> = members.iter().copied().collect();
    let mut state = BTreeMap::new();
    let mut kept: BTreeMap<usize, Vec<usize>> =
        members.iter().map(|member| (*member, Vec::new())).collect();
    let mut carry = BTreeSet::new();

    fn visit(
        node: usize,
        member_set: &BTreeSet<usize>,
        adj: &[Vec<usize>],
        state: &mut BTreeMap<usize, u8>,
        kept: &mut BTreeMap<usize, Vec<usize>>,
        carry: &mut BTreeSet<(usize, usize)>,
    ) {
        state.insert(node, 1);
        for &next in &adj[node] {
            if !member_set.contains(&next) {
                continue;
            }
            match state.get(&next).copied().unwrap_or(0) {
                0 => {
                    kept.entry(node).or_default().push(next);
                    visit(next, member_set, adj, state, kept, carry);
                }
                1 => {
                    // Back-edge: carried into the next round.
                    carry.insert((node, next));
                }
                _ => {
                    kept.entry(node).or_default().push(next);
                }
            }
        }
        state.insert(node, 2);
    }

    // Start at external entry targets, then cycle-policy nodes, so the DFS
    // back-edge is the loop tail -> loop head edge rather than an arbitrary
    // branch or external edge.
    let mut roots: Vec<usize> = external_targets.iter().copied().collect();
    roots.sort_by(|a, b| names[*a].cmp(&names[*b]));
    let mut policy: Vec<usize> = members
        .iter()
        .copied()
        .filter(|member| spec.steps[&names[*member]].cycle.is_some())
        .collect();
    policy.sort_by(|a, b| names[*a].cmp(&names[*b]));
    for member in policy {
        if !roots.contains(&member) {
            roots.push(member);
        }
    }
    for &member in members {
        if !roots.contains(&member) {
            roots.push(member);
        }
    }
    for member in roots {
        if state.get(&member).copied().unwrap_or(0) == 0 {
            visit(member, &member_set, adj, &mut state, &mut kept, &mut carry);
        }
    }

    let mut indegree: BTreeMap<usize, usize> = members.iter().map(|member| (*member, 0)).collect();
    for edges in kept.values() {
        for &target in edges {
            *indegree.entry(target).or_default() += 1;
        }
    }
    let mut queue: VecDeque<usize> = members
        .iter()
        .copied()
        .filter(|member| indegree.get(member).copied().unwrap_or(0) == 0)
        .collect();
    let mut levels: Vec<Vec<usize>> = Vec::new();
    while !queue.is_empty() {
        let mut level = Vec::new();
        for _ in 0..queue.len() {
            if let Some(node) = queue.pop_front() {
                level.push(node);
                if let Some(targets) = kept.get(&node) {
                    for &target in targets {
                        let entry = indegree.entry(target).or_default();
                        *entry = entry.saturating_sub(1);
                        if *entry == 0 {
                            queue.push_back(target);
                        }
                    }
                }
            }
        }
        level.sort_by(|a, b| names[*a].cmp(&names[*b]));
        levels.push(level);
    }

    if levels.is_empty() {
        levels.push(members.to_vec());
    }
    (levels, carry)
}

/// Tarjan's strongly connected components. Deterministic for a sorted graph.
fn strongly_connected(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State {
        index: usize,
        indices: Vec<Option<usize>>,
        low: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }

    fn connect(node: usize, adj: &[Vec<usize>], state: &mut State) {
        state.indices[node] = Some(state.index);
        state.low[node] = state.index;
        state.index += 1;
        state.stack.push(node);
        state.on_stack[node] = true;

        for &next in &adj[node] {
            if state.indices[next].is_none() {
                connect(next, adj, state);
                state.low[node] = state.low[node].min(state.low[next]);
            } else if state.on_stack[next]
                && let Some(next_index) = state.indices[next]
            {
                state.low[node] = state.low[node].min(next_index);
            }
        }

        if state.low[node] == state.indices[node].unwrap_or(usize::MAX) {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            component.sort_unstable();
            state.components.push(component);
        }
    }

    let mut state = State {
        index: 0,
        indices: vec![None; adj.len()],
        low: vec![0; adj.len()],
        stack: Vec::new(),
        on_stack: vec![false; adj.len()],
        components: Vec::new(),
    };
    for node in 0..adj.len() {
        if state.indices[node].is_none() {
            connect(node, adj, &mut state);
        }
    }
    state.components
}

/// Validate await/emit declarations and enforce the same-group visibility rule.
///
/// A group commits atomically, so events emitted by a node in group `N` only
/// become visible to awaits in group `N+1` or later. Within a materialised
/// cycle round the same rule applies: an await may only consume internal
/// events emitted by a strictly earlier group of that round.
fn validate_awaits(spec: &WorkflowSpec, nodes: &[PlanNode]) -> Result<(), DomainError> {
    for (name, step) in &spec.steps {
        match (step.kind, &step.await_spec) {
            (StepKind::Await, None) => {
                return Err(DomainError::AwaitRequiresSpec(name.clone()));
            }
            (StepKind::Await, Some(await_spec)) => {
                if step.retry != RetryPolicy::default() {
                    return Err(DomainError::AwaitRetryNotAllowed(name.clone()));
                }
                for (present, field) in [
                    (step.exec.is_some(), "exec"),
                    (step.agent.is_some(), "agent"),
                    (step.improve.is_some(), "improve"),
                    (step.gateway.is_some(), "gateway"),
                ] {
                    if present {
                        return Err(DomainError::AwaitPayloadNotAllowed {
                            step: name.clone(),
                            field: field.to_owned(),
                        });
                    }
                }
                validate_workflow_event(&await_spec.event, &serde_json::Value::Null)?;
            }
            (_, Some(_)) => return Err(DomainError::AwaitOnNonAwait(name.clone())),
            (_, None) => {}
        }
        for emit in &step.emit {
            validate_workflow_event(
                &emit.name,
                emit.payload.as_ref().unwrap_or(&serde_json::Value::Null),
            )?;
        }
    }

    let mut emitters: BTreeMap<&str, Vec<&PlanNode>> = BTreeMap::new();
    for node in nodes {
        let step = &spec.steps[&node.step];
        for emit in &step.emit {
            if !emit.external {
                emitters.entry(emit.name.as_str()).or_default().push(node);
            }
        }
    }

    for node in nodes {
        let step = &spec.steps[&node.step];
        let Some(await_spec) = &step.await_spec else {
            continue;
        };
        if await_spec.source() == EventSource::External {
            continue;
        }
        let Some(candidates) = emitters.get(await_spec.event.as_str()) else {
            continue;
        };
        for emitter in candidates {
            if emitter.round != node.round {
                continue;
            }
            if emitter.group >= node.group {
                return Err(DomainError::AwaitEmitterNotEarlier {
                    await_step: node.step.clone(),
                    event: await_spec.event.clone(),
                    emitter: emitter.step.clone(),
                });
            }
        }
    }
    Ok(())
}
