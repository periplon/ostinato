#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::{
    AuditEvent, BranchSpec, CycleSpec, DomainError, EdgeOrigin, GatewaySpec, JoinMode, NewEvent,
    PlanEdge, RetryPolicy, SplitMode, StepKind, StepSpec, WorkflowSpec, audit, build_plan,
};
use serde_json::json;

fn step(name: &str) -> StepSpec {
    StepSpec {
        name: name.to_owned(),
        kind: StepKind::Noop,
        needs: Vec::new(),
        retry: RetryPolicy::default(),
        cycle: None,
        exec: None,
        agent: None,
        improve: None,
        gateway: None,
        continue_on_error: false,
        timeout_ms: None,
    }
}

#[test]
fn independent_steps_share_a_parallel_group() {
    let mut steps = BTreeMap::new();
    steps.insert("a".to_owned(), step("a"));
    steps.insert("b".to_owned(), step("b"));
    let mut c = step("c");
    c.needs = vec!["a".to_owned(), "b".to_owned()];
    steps.insert("c".to_owned(), c);
    let spec = WorkflowSpec {
        name: "dag".to_owned(),
        max_parallel: 2,
        steps,
    };
    let plan = build_plan(&spec).expect("plan");
    assert_eq!(plan.nodes.len(), 3);
    assert_eq!(plan.nodes[0].group, plan.nodes[1].group);
    assert!(plan.nodes[2].group > plan.nodes[0].group);
    assert!(!plan.has_cycles);
}

#[test]
fn cycles_expand_into_bounded_rounds() {
    let mut steps = BTreeMap::new();
    let mut a = step("a");
    a.needs = vec!["b".to_owned()];
    a.cycle = Some(CycleSpec {
        max_rounds: 3,
        until: Some("steps.a.score >= 0.9".to_owned()),
    });
    let mut b = step("b");
    b.needs = vec!["a".to_owned()];
    steps.insert("a".to_owned(), a);
    steps.insert("b".to_owned(), b);
    let spec = WorkflowSpec {
        name: "cycle".to_owned(),
        max_parallel: 1,
        steps,
    };
    let plan = build_plan(&spec).expect("plan");
    assert!(plan.has_cycles);
    assert_eq!(plan.nodes.len(), 6);
    assert_eq!(plan.nodes[0].round, 0);
    assert_eq!(plan.nodes[2].round, 1);
    assert_eq!(plan.nodes[5].round, 2);
    assert!(plan.nodes[1].is_round_end);
    assert_eq!(plan.nodes[0].cycle_max_rounds, 3);
}

#[test]
fn cycles_require_an_explicit_policy() {
    let mut steps = BTreeMap::new();
    let mut a = step("a");
    a.needs = vec!["b".to_owned()];
    let mut b = step("b");
    b.needs = vec!["a".to_owned()];
    steps.insert("a".to_owned(), a);
    steps.insert("b".to_owned(), b);
    let spec = WorkflowSpec {
        name: "bad-cycle".to_owned(),
        max_parallel: 1,
        steps,
    };
    let error = build_plan(&spec).expect_err("cycle must fail");
    assert!(matches!(error, DomainError::CycleWithoutPolicy { .. }));
}

#[test]
fn audit_chain_detects_tampering() {
    let first = audit::seal(
        "run",
        0,
        "2026-01-01T00:00:00Z",
        "run.started",
        None,
        &json!({"workflow": "demo"}),
        audit::GENESIS_HASH,
    )
    .expect("seal");
    let second = audit::seal(
        "run",
        1,
        "2026-01-01T00:00:01Z",
        "step.succeeded",
        Some("a"),
        &json!({"attempts": 1}),
        &first.hash,
    )
    .expect("seal");
    let events: Vec<AuditEvent> = vec![first.clone(), second.clone()];
    audit::verify_chain(&events).expect("valid chain");

    let mut tampered = events.clone();
    tampered[1].data = json!({"attempts": 99});
    let error = audit::verify_chain(&tampered).expect_err("tamper must fail");
    assert!(matches!(error, DomainError::InvalidAuditChain { .. }));

    let mut reordered = events;
    reordered.swap(0, 1);
    assert!(audit::verify_chain(&reordered).is_err());
}

#[test]
fn retry_backoff_is_capped_and_deterministic() {
    let policy = RetryPolicy {
        max_retries: 10,
        base_delay_ms: 100,
        factor: 2.0,
        max_delay_ms: 1_000,
        jitter: false,
    };
    assert_eq!(policy.delay_for(0, 7).as_millis(), 100);
    assert_eq!(policy.delay_for(1, 7).as_millis(), 200);
    assert_eq!(policy.delay_for(10, 7).as_millis(), 1_000);
    assert_eq!(policy.delay_for(3, 7), policy.delay_for(3, 7));
}

#[test]
fn new_event_helper_is_ergonomic() {
    let event = NewEvent::new("run.started", None, json!({}));
    assert_eq!(event.kind, "run.started");
}

fn gateway_step(name: &str, gateway: GatewaySpec) -> StepSpec {
    let mut spec = step(name);
    spec.kind = StepKind::Gateway;
    spec.gateway = Some(gateway);
    spec
}

fn split_gateway(
    mode: SplitMode,
    branches: &[(&str, Option<&str>)],
    default: Option<&str>,
) -> GatewaySpec {
    let mut gateway = GatewaySpec {
        split: Some(mode),
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
    gateway
}

fn join_gateway(mode: JoinMode) -> GatewaySpec {
    GatewaySpec {
        split: None,
        join: Some(mode),
        branches: BTreeMap::new(),
        default: None,
    }
}

fn gateway_workflow() -> WorkflowSpec {
    let mut steps = BTreeMap::new();
    steps.insert("test".to_owned(), step("test"));
    let mut route = gateway_step(
        "route",
        split_gateway(
            SplitMode::Xor,
            &[("deployFast", Some("params.fast")), ("deploySlow", None)],
            Some("deploySlow"),
        ),
    );
    route.needs = vec!["test".to_owned()];
    steps.insert("route".to_owned(), route);
    steps.insert("deployFast".to_owned(), step("deployFast"));
    steps.insert("deploySlow".to_owned(), step("deploySlow"));
    let mut merge = gateway_step("merge", join_gateway(JoinMode::Or));
    merge.needs = vec!["deployFast".to_owned(), "deploySlow".to_owned()];
    steps.insert("merge".to_owned(), merge);
    let mut notify = step("notify");
    notify.needs = vec!["merge".to_owned()];
    steps.insert("notify".to_owned(), notify);
    WorkflowSpec {
        name: "gateways".to_owned(),
        max_parallel: 4,
        steps,
    }
}

#[test]
fn gateway_plan_orders_branches_and_joins() {
    let plan = build_plan(&gateway_workflow()).expect("plan");
    assert!(plan.has_gateways);
    assert!(!plan.has_cycles);
    let index = |name: &str| {
        plan.nodes
            .iter()
            .position(|node| node.step == name)
            .expect("node")
    };
    assert!(index("route") < index("deployFast"));
    assert!(index("route") < index("deploySlow"));
    assert!(index("merge") > index("deployFast"));
    assert!(index("merge") > index("deploySlow"));
    assert!(index("notify") > index("merge"));

    let fast = &plan.nodes[index("deployFast")];
    assert_eq!(
        fast.incoming,
        vec![PlanEdge {
            from: "route".to_owned(),
            origin: EdgeOrigin::Branch {
                gateway: "route".to_owned(),
                branch: "deployFast".to_owned(),
            },
        }]
    );
    assert_eq!(plan.nodes[index("route")].incoming.len(), 1);
    assert_eq!(plan.nodes[index("merge")].incoming.len(), 2);
}

#[test]
fn legacy_plan_shape_is_unchanged_without_gateways() {
    let mut steps = BTreeMap::new();
    steps.insert("a".to_owned(), step("a"));
    let mut b = step("b");
    b.needs = vec!["a".to_owned()];
    steps.insert("b".to_owned(), b);
    let plan = build_plan(&WorkflowSpec {
        name: "legacy".to_owned(),
        max_parallel: 1,
        steps,
    })
    .expect("plan");
    assert!(!plan.has_gateways);
    assert!(plan.nodes.iter().all(|node| node.gateway.is_none()));
    assert_eq!(
        plan.nodes[1].incoming,
        vec![PlanEdge {
            from: "a".to_owned(),
            origin: EdgeOrigin::Dependency,
        }]
    );
}

#[test]
fn gateway_and_cycle_combination_is_rejected() {
    let mut steps = BTreeMap::new();
    let mut a = step("a");
    a.needs = vec!["b".to_owned()];
    a.cycle = Some(CycleSpec {
        max_rounds: 2,
        until: None,
    });
    let mut b = step("b");
    b.needs = vec!["a".to_owned()];
    steps.insert("a".to_owned(), a);
    steps.insert("b".to_owned(), b);
    steps.insert(
        "gate".to_owned(),
        gateway_step("gate", split_gateway(SplitMode::And, &[("c", None)], None)),
    );
    steps.insert("c".to_owned(), step("c"));
    let error = build_plan(&WorkflowSpec {
        name: "both".to_owned(),
        max_parallel: 1,
        steps,
    })
    .expect_err("must reject");
    assert!(matches!(error, DomainError::GatewayCycleUnsupported { .. }));
}

#[test]
fn implicit_join_with_gateways_is_rejected() {
    let mut spec = gateway_workflow();
    let mut extra = step("extra");
    extra.needs = vec!["test".to_owned(), "deployFast".to_owned()];
    spec.steps.insert("extra".to_owned(), extra);
    let error = build_plan(&spec).expect_err("must reject");
    assert_eq!(
        error,
        DomainError::ImplicitJoinWithGateways("extra".to_owned())
    );
}

#[test]
fn unknown_branch_target_is_rejected() {
    let mut spec = gateway_workflow();
    spec.steps.insert(
        "route".to_owned(),
        gateway_step(
            "route",
            split_gateway(SplitMode::And, &[("missing", None)], None),
        ),
    );
    let error = build_plan(&spec).expect_err("must reject");
    assert!(matches!(
        error,
        DomainError::GatewayUnknownBranchTarget { .. }
    ));
}

#[test]
fn branch_target_with_needs_is_rejected() {
    let mut spec = gateway_workflow();
    spec.steps.get_mut("deployFast").expect("step").needs = vec!["test".to_owned()];
    let error = build_plan(&spec).expect_err("must reject");
    assert!(matches!(
        error,
        DomainError::GatewayBranchTargetHasNeeds { .. }
    ));
}

#[test]
fn join_with_branches_is_rejected() {
    let mut spec = gateway_workflow();
    let mut merge = join_gateway(JoinMode::Or);
    merge
        .branches
        .insert("x".to_owned(), BranchSpec { condition: None });
    spec.steps
        .insert("merge".to_owned(), gateway_step("merge", merge));
    spec.steps.get_mut("merge").expect("merge").needs = vec!["deployFast".to_owned()];
    let error = build_plan(&spec).expect_err("must reject");
    assert_eq!(
        error,
        DomainError::GatewayJoinHasBranches("merge".to_owned())
    );
}

#[test]
fn split_and_join_together_is_rejected() {
    let mut spec = gateway_workflow();
    let mut gateway = split_gateway(SplitMode::And, &[("deployFast", None)], None);
    gateway.join = Some(JoinMode::And);
    spec.steps
        .insert("route".to_owned(), gateway_step("route", gateway));
    let error = build_plan(&spec).expect_err("must reject");
    assert_eq!(error, DomainError::GatewaySplitAndJoin("route".to_owned()));
}

#[test]
fn invalid_default_is_rejected() {
    let mut spec = gateway_workflow();
    let gateway = split_gateway(
        SplitMode::Xor,
        &[("deployFast", Some("params.fast"))],
        Some("nope"),
    );
    spec.steps
        .insert("route".to_owned(), gateway_step("route", gateway));
    let error = build_plan(&spec).expect_err("must reject");
    assert!(matches!(error, DomainError::GatewayInvalidDefault { .. }));
}

#[test]
fn legacy_step_spec_deserializes_without_gateway_and_reserializes_identically() {
    let legacy = r#"{"name":"a","kind":"noop","needs":[],"retry":{"max_retries":0,"base_delay_ms":100,"factor":2.0,"max_delay_ms":30000,"jitter":true},"cycle":null,"exec":null,"agent":null,"improve":null,"continue_on_error":false,"timeout_ms":null}"#;
    let step: StepSpec = serde_json::from_str(legacy).expect("deserialize legacy step");
    assert!(step.gateway.is_none());
    let serialized = serde_json::to_string(&step).expect("serialize");
    assert_eq!(serialized, legacy);
    assert!(!serialized.contains("gateway"));
}

#[test]
fn legacy_state_digest_is_stable_across_the_gateway_change() {
    let legacy_spec = r#"{"name":"legacy","max_parallel":2,"steps":{"a":{"name":"a","kind":"noop","needs":[],"retry":{"max_retries":0,"base_delay_ms":100,"factor":2.0,"max_delay_ms":30000,"jitter":true},"cycle":null,"exec":null,"agent":null,"improve":null,"continue_on_error":false,"timeout_ms":null},"b":{"name":"b","kind":"exec","needs":["a"],"retry":{"max_retries":0,"base_delay_ms":100,"factor":2.0,"max_delay_ms":30000,"jitter":true},"cycle":null,"exec":{"cmd":["sh","-c","echo b"],"cwd":null,"env":{},"timeout_ms":null},"agent":null,"improve":null,"continue_on_error":false,"timeout_ms":null}}}"#;
    // Captured from the pre-gateway implementation.
    let expected = "a41013b4e440b0441253a7b4166e9a3db7fd08fa039d19a98df5e91803ffee4b";
    let spec: WorkflowSpec = serde_json::from_str(legacy_spec).expect("deserialize legacy spec");
    assert_eq!(
        serde_json::to_string(&spec).expect("serialize"),
        legacy_spec,
        "gateway must not appear in legacy serialization"
    );
    assert_eq!(
        audit::state_digest(&spec, &json!({"x": 1}), "legacy.cue", 2, &[]),
        expected
    );
}
