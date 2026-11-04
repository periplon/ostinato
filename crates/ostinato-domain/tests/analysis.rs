#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::{
    BranchSpec, GatewaySpec, JoinMode, SplitMode, StepKind, StepSpec, WorkflowSpec, analyze,
    build_plan,
};

fn step(name: &str) -> StepSpec {
    StepSpec::new(name)
}

fn split(name: &str, branches: &[&str]) -> StepSpec {
    let mut spec = step(name);
    spec.kind = StepKind::Gateway;
    spec.gateway = Some(GatewaySpec {
        split: Some(SplitMode::Xor),
        branches: branches
            .iter()
            .enumerate()
            .map(|(index, branch)| {
                (
                    (*branch).to_owned(),
                    BranchSpec {
                        condition: (index != 0).then_some("true".to_owned()),
                    },
                )
            })
            .collect(),
        default: Some(branches[0].to_owned()),
        ..GatewaySpec::default()
    });
    spec
}

fn join(name: &str, mode: JoinMode, needs: &[&str]) -> StepSpec {
    let mut spec = step(name);
    spec.kind = StepKind::Gateway;
    spec.needs = needs.iter().map(ToString::to_string).collect();
    spec.gateway = Some(GatewaySpec {
        join: Some(mode),
        ..GatewaySpec::default()
    });
    spec
}

fn workflow(steps: Vec<StepSpec>) -> WorkflowSpec {
    WorkflowSpec {
        name: "analysis".to_owned(),
        max_parallel: 2,
        steps: steps
            .into_iter()
            .map(|step| (step.name.clone(), step))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn xor_branches_feeding_an_and_join_are_reported() {
    let spec = workflow(vec![
        split("route", &["a", "b"]),
        step("a"),
        step("b"),
        join("merge", JoinMode::And, &["a", "b"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayJoinUnreachable"),
        "{:?}",
        report.findings
    );
}

#[test]
fn xor_branches_feeding_an_xor_join_are_valid() {
    let spec = workflow(vec![
        split("route", &["a", "b"]),
        step("a"),
        step("b"),
        join("merge", JoinMode::Xor, &["a", "b"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayJoinUnreachable"),
        "{:?}",
        report.findings
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayXorJoinAmbiguityPotential"),
        "{:?}",
        report.findings
    );
}

#[test]
fn shared_branch_target_is_reported() {
    let left = split("left", &["c"]);
    let right = split("right", &["c"]);
    let target = split("c", &["d"]);
    let spec = workflow(vec![left, right, target, step("d")]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewaySharedBranchTarget"),
        "{:?}",
        report.findings
    );
}

#[test]
fn missing_default_is_a_note() {
    let mut spec = split("route", &["a"]);
    if let Some(gateway) = &mut spec.gateway {
        gateway.default = None;
        gateway.branches.insert(
            "a".to_owned(),
            BranchSpec {
                condition: Some("params.x".to_owned()),
            },
        );
    }
    let spec = workflow(vec![spec, step("a")]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayNoDefaultFallback"),
        "{:?}",
        report.findings
    );
}

#[test]
fn n_of_one_over_exclusive_branches_is_reachable() {
    let spec = workflow(vec![split("route", &["a", "b"]), step("a"), step("b"), {
        let mut quorum = join("quorum", JoinMode::NOf, &["a", "b"]);
        if let Some(gateway) = &mut quorum.gateway {
            gateway.n = Some(1);
        }
        quorum
    }]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayJoinUnreachable"),
        "{:?}",
        report.findings
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayQuorumUnreachable"),
        "{:?}",
        report.findings
    );
}

#[test]
fn xor_join_over_non_exclusive_inputs_is_reported() {
    let mut route = step("route");
    route.kind = StepKind::Gateway;
    route.gateway = Some(GatewaySpec {
        split: Some(SplitMode::Or),
        branches: ["a", "b"]
            .into_iter()
            .map(|branch| {
                (
                    branch.to_owned(),
                    BranchSpec {
                        condition: Some("true".to_owned()),
                    },
                )
            })
            .collect(),
        ..GatewaySpec::default()
    });
    let spec = workflow(vec![
        route,
        step("a"),
        step("b"),
        join("merge", JoinMode::Xor, &["a", "b"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayXorJoinAmbiguityPotential"),
        "{:?}",
        report.findings
    );
}

#[test]
fn branch_that_does_not_reach_a_join_is_reported() {
    let spec = workflow(vec![
        split("route", &["a", "b"]),
        step("a"),
        step("b"),
        join("merge", JoinMode::Or, &["a"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayBranchEscapesJoin"),
        "{:?}",
        report.findings
    );
}

#[test]
fn nested_xor_exclusivity_is_understood() {
    let spec = workflow(vec![
        split("outer", &["inner", "a2"]),
        split("inner", &["b1", "b2"]),
        step("a2"),
        step("b1"),
        step("b2"),
        join("merge", JoinMode::Xor, &["b1", "b2", "a2"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayXorJoinAmbiguityPotential"),
        "{:?}",
        report.findings
    );
}

#[test]
fn sibling_branches_with_separate_joins_are_reported() {
    let spec = workflow(vec![
        split("route", &["a", "b"]),
        step("a"),
        step("b"),
        join("ja", JoinMode::Or, &["a"]),
        join("jb", JoinMode::Or, &["b"]),
    ]);
    let plan = build_plan(&spec).expect("plan");
    let report = analyze(&spec, &plan);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "GatewayBranchEscapesJoin"),
        "{:?}",
        report.findings
    );
}
