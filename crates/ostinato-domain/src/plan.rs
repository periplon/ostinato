use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::workflow::{GatewaySpec, StepKind, WorkflowSpec};

/// Hard cap on materialised plan nodes. Cycles expand eagerly; this prevents
/// a workflow file from exhausting memory during planning.
pub const MAX_PLAN_NODES: usize = 100_000;

/// Where an incoming plan edge came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeOrigin {
    /// An explicit `needs` dependency.
    Dependency,
    /// An implicit edge created by a split gateway branch.
    Branch { gateway: String, branch: String },
}

/// One incoming edge of a plan node. Metadata only; not persisted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlanEdge {
    pub from: String,
    pub origin: EdgeOrigin,
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
    /// True when this node belongs to a materialised cycle. Activation is
    /// bypassed for cycle members because their internal back-edges have no
    /// record until the round completes.
    #[serde(default)]
    pub in_cycle: bool,
    /// Incoming edges in deterministic `(from, origin)` order.
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

    if has_gateways {
        let cyclic_steps: Vec<String> = (0..names.len())
            .filter(|node| is_cyclic(comp_of[*node], &comp_nodes, &has_self_edge, &comp_of, &adj))
            .map(|node| names[node].clone())
            .collect();
        if !cyclic_steps.is_empty() {
            return Err(DomainError::GatewayCycleUnsupported {
                steps: cyclic_steps,
            });
        }
        for (node, name) in names.iter().enumerate() {
            let is_gateway = spec.steps[name].kind == StepKind::Gateway;
            if !is_gateway && incoming[node].len() > 1 {
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
            let levels = inner_levels(members, &adj, &names);
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
                            incoming: incoming[node].clone(),
                            gateway: spec.steps[&names[node]].gateway.clone(),
                        });
                    }
                    group += 1;
                }
            }
        }
    }

    Ok(Plan {
        groups: group,
        has_cycles,
        has_gateways,
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
            (None, Some(_)) => {
                if step.needs.is_empty() {
                    return Err(DomainError::GatewayJoinWithoutNeeds(name.clone()));
                }
                if !gateway.branches.is_empty() || gateway.default.is_some() {
                    return Err(DomainError::GatewayJoinHasBranches(name.clone()));
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

/// Remove DFS back-edges, then compute Kahn levels of the remaining DAG.
fn inner_levels(members: &[usize], adj: &[Vec<usize>], names: &[String]) -> Vec<Vec<usize>> {
    let member_set: BTreeSet<usize> = members.iter().copied().collect();
    let mut state = BTreeMap::new();
    let mut kept: BTreeMap<usize, Vec<usize>> =
        members.iter().map(|member| (*member, Vec::new())).collect();

    fn visit(
        node: usize,
        member_set: &BTreeSet<usize>,
        adj: &[Vec<usize>],
        state: &mut BTreeMap<usize, u8>,
        kept: &mut BTreeMap<usize, Vec<usize>>,
    ) {
        state.insert(node, 1);
        for &next in &adj[node] {
            if !member_set.contains(&next) {
                continue;
            }
            match state.get(&next).copied().unwrap_or(0) {
                0 => {
                    kept.entry(node).or_default().push(next);
                    visit(next, member_set, adj, state, kept);
                }
                1 => {
                    // Back-edge: removed to make this round a DAG.
                }
                _ => {
                    kept.entry(node).or_default().push(next);
                }
            }
        }
        state.insert(node, 2);
    }

    for &member in members {
        if state.get(&member).copied().unwrap_or(0) == 0 {
            visit(member, &member_set, adj, &mut state, &mut kept);
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
    levels
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
