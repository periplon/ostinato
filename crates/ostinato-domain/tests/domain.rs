#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::{
    ActivationScope, AuditEvent, BranchSpec, CycleSpec, DomainError, EdgeOrigin, EdgeScope,
    GatewaySpec, JoinMode, NewEvent, PlanEdge, RetryPolicy, SplitMode, StepKind, StepSpec,
    WorkflowSpec, audit, build_plan,
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
        await_spec: None,
        emit: Vec::new(),
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
    gateway
}

fn join_gateway(mode: JoinMode) -> GatewaySpec {
    GatewaySpec {
        split: None,
        join: Some(mode),
        branches: BTreeMap::new(),
        default: None,
        ..GatewaySpec::default()
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
            scope: EdgeScope::SameRound,
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
            scope: EdgeScope::SameRound,
            origin: EdgeOrigin::Dependency,
        }]
    );
}

#[test]
fn gateway_outside_a_legacy_cycle_is_allowed() {
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
    let plan = build_plan(&WorkflowSpec {
        name: "both".to_owned(),
        max_parallel: 1,
        steps,
    })
    .expect("gateway outside a legacy cycle is allowed");
    assert!(!plan.has_gateway_cycles);
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

#[test]
fn n_of_and_expression_joins_are_materialised() {
    let mut steps = BTreeMap::new();
    steps.insert("a".to_owned(), step("a"));
    steps.insert("b".to_owned(), step("b"));
    let mut quorum = gateway_step("quorum", join_gateway(JoinMode::NOf));
    quorum.needs = vec!["a".to_owned(), "b".to_owned()];
    if let Some(gateway) = &mut quorum.gateway {
        gateway.n = Some(2);
    }
    steps.insert("quorum".to_owned(), quorum);
    let mut expression = gateway_step("expression", join_gateway(JoinMode::Expression));
    expression.needs = vec!["a".to_owned(), "b".to_owned()];
    if let Some(gateway) = &mut expression.gateway {
        gateway.expression = Some("edges.a".to_owned());
    }
    steps.insert("expression".to_owned(), expression);
    let plan = build_plan(&WorkflowSpec {
        name: "joins".to_owned(),
        max_parallel: 2,
        steps,
    })
    .expect("plan");
    assert!(plan.nodes.iter().any(|node| node.step == "quorum"));
    assert!(plan.nodes.iter().any(|node| node.step == "expression"));
}

#[test]
fn gateway_cycle_plan_marks_round_scope_and_carry_edges() {
    let mut steps = BTreeMap::new();
    let mut loop_step = step("loop");
    loop_step.needs = vec!["tail".to_owned()];
    loop_step.cycle = Some(CycleSpec {
        max_rounds: 2,
        until: None,
    });
    steps.insert("loop".to_owned(), loop_step);
    let mut split = gateway_step(
        "split",
        split_gateway(
            SplitMode::Xor,
            &[("work", Some("true")), ("idle", None)],
            Some("idle"),
        ),
    );
    split.needs = vec!["loop".to_owned()];
    steps.insert("split".to_owned(), split);
    steps.insert("work".to_owned(), step("work"));
    steps.insert("idle".to_owned(), step("idle"));
    let mut merge = gateway_step("merge", join_gateway(JoinMode::Or));
    merge.needs = vec!["work".to_owned(), "idle".to_owned()];
    steps.insert("merge".to_owned(), merge);
    let mut tail = step("tail");
    tail.needs = vec!["merge".to_owned()];
    steps.insert("tail".to_owned(), tail);

    let plan = build_plan(&WorkflowSpec {
        name: "cycle".to_owned(),
        max_parallel: 1,
        steps,
    })
    .expect("plan");
    assert!(plan.has_gateway_cycles);
    assert_eq!(plan.cycle_info.len(), 1);
    assert_eq!(plan.cycle_info[0].entries, vec!["loop".to_owned()]);
    assert_eq!(plan.cycle_info[0].carry.len(), 1);
    assert_eq!(plan.cycle_info[0].carry[0].from, "tail");
    assert_eq!(plan.cycle_info[0].carry[0].to, "loop");
    assert!(
        plan.nodes
            .iter()
            .filter(|node| node.step == "split")
            .all(|node| node.activation == ActivationScope::RoundCycle)
    );
}

#[test]
fn externally_entered_gateway_cycle_keeps_the_external_edge_round_agnostic() {
    let mut steps = BTreeMap::new();
    steps.insert("x".to_owned(), step("x"));
    let mut loop_step = step("loop");
    loop_step.needs = vec!["x".to_owned(), "tail".to_owned()];
    steps.insert("loop".to_owned(), loop_step);
    let mut split = gateway_step(
        "split",
        split_gateway(SplitMode::And, &[("work", None)], None),
    );
    split.needs = vec!["loop".to_owned()];
    steps.insert("split".to_owned(), split);
    steps.insert("work".to_owned(), step("work"));
    let mut merge = gateway_step("merge", join_gateway(JoinMode::And));
    merge.needs = vec!["work".to_owned()];
    steps.insert("merge".to_owned(), merge);
    let mut tail = step("tail");
    tail.needs = vec!["merge".to_owned()];
    tail.cycle = Some(CycleSpec {
        max_rounds: 3,
        until: None,
    });
    steps.insert("tail".to_owned(), tail);

    let plan = build_plan(&WorkflowSpec {
        name: "external-cycle".to_owned(),
        max_parallel: 1,
        steps,
    })
    .expect("plan");
    assert_eq!(plan.cycle_info.len(), 1);
    assert_eq!(plan.cycle_info[0].carry[0].from, "tail");
    assert_eq!(plan.cycle_info[0].carry[0].to, "loop");
    assert!(
        plan.cycle_info[0]
            .external_incoming
            .iter()
            .any(|edge| edge.from == "x")
    );
    assert!(
        plan.nodes
            .iter()
            .filter(|node| node.step == "loop")
            .all(|node| {
                node.incoming
                    .iter()
                    .any(|edge| edge.from == "x" && edge.scope == EdgeScope::SameRound)
            })
    );
}

#[test]
fn legacy_agent_specs_keep_their_state_digest_bytes() {
    use ostinato_domain::{AgentSpec, StepSpec, WorkflowSpec, audit};
    let mut steps = std::collections::BTreeMap::new();
    steps.insert(
        "review".to_owned(),
        StepSpec {
            name: "review".to_owned(),
            kind: ostinato_domain::StepKind::Agent,
            needs: Vec::new(),
            retry: ostinato_domain::RetryPolicy::default(),
            cycle: None,
            exec: None,
            agent: Some(AgentSpec {
                prompt: "hello".to_owned(),
                system: None,
                model: None,
                command: None,
                ..AgentSpec::default()
            }),
            improve: None,
            gateway: None,
            await_spec: None,
            emit: Vec::new(),
            continue_on_error: false,
            timeout_ms: None,
        },
    );
    let spec = WorkflowSpec {
        name: "legacy".to_owned(),
        max_parallel: 1,
        steps,
    };
    let params = serde_json::json!({});
    let before = audit::state_digest(&spec, &params, "legacy.cue", 0, &[]);
    let json = serde_json::to_string(&spec).expect("serialize");
    assert!(!json.contains("provider"), "{json}");
    assert!(!json.contains("isolation"), "{json}");
    assert!(!json.contains("definition"), "{json}");
    let restored: WorkflowSpec = serde_json::from_str(&json).expect("deserialize");
    let after = audit::state_digest(&restored, &params, "legacy.cue", 0, &[]);
    assert_eq!(before, after);
}

#[test]
fn agent_trace_hash_is_deterministic_and_content_sensitive() {
    use ostinato_domain::AgentTrace;
    let mut trace = AgentTrace {
        run_id: "r".to_owned(),
        step: "s".to_owned(),
        round: 0,
        attempt: 0,
        definition: None,
        provider: "command".to_owned(),
        model: None,
        prompt: "p".to_owned(),
        prompt_sha256: ostinato_domain::sha256_hex(b"p"),
        system: None,
        argv: vec!["agent".to_owned()],
        env_keys: Vec::new(),
        env_hashes: std::collections::BTreeMap::new(),
        cwd: "/tmp".to_owned(),
        isolation: "none".to_owned(),
        isolation_path: None,
        isolation_base: None,
        started_at: "a".to_owned(),
        finished_at: "b".to_owned(),
        duration_ms: 1,
        success: true,
        exit_code: Some(0),
        timed_out: false,
        signal: None,
        pid: None,
        program: Some("agent".to_owned()),
        ostinato_version: "v".to_owned(),
        stdout_truncated: false,
        stderr_truncated: false,
        stdin_sha256: None,
        error: None,
        stdout: "ok".to_owned(),
        stderr: String::new(),
        transcript: None,
        artifacts: Vec::new(),
        capture_warnings: Vec::new(),
        capture_error: None,
        cleanup_error: None,
    };
    let first = trace.trace_hash();
    assert_eq!(first, trace.clone().trace_hash());
    trace.stdout = "different".to_owned();
    assert_ne!(first, trace.trace_hash());
}

#[test]
fn legacy_step_specs_do_not_gain_await_or_emit_bytes() {
    let spec = step("legacy");
    let value = serde_json::to_value(&spec).expect("serialize step");
    assert!(value.get("await").is_none());
    assert!(value.get("emit").is_none());
    let expected = json!({
        "name": "legacy",
        "kind": "noop",
        "needs": [],
        "retry": {
            "max_retries": 0,
            "base_delay_ms": 100,
            "factor": 2.0,
            "max_delay_ms": 30000,
            "jitter": true
        },
        "cycle": null,
        "exec": null,
        "agent": null,
        "improve": null,
        "continue_on_error": false,
        "timeout_ms": null
    });
    assert_eq!(value, expected);
}

#[test]
fn await_and_emit_serialize_with_the_documented_names() {
    use ostinato_domain::{AwaitSpec, EmitSpec, EventSource};
    let mut spec = step("waiter");
    spec.kind = StepKind::Await;
    spec.await_spec = Some(AwaitSpec {
        event: "deploy".to_owned(),
        source: Some(EventSource::External),
        timeout_ms: Some(1000),
    });
    spec.emit = vec![EmitSpec {
        name: "done".to_owned(),
        external: true,
        payload: Some(json!({"ok": true})),
    }];
    let value = serde_json::to_value(&spec).expect("serialize step");
    assert_eq!(value["kind"], "await");
    assert_eq!(value["await"]["event"], "deploy");
    assert_eq!(value["await"]["source"], "external");
    assert_eq!(value["emit"][0]["name"], "done");
    assert_eq!(value["emit"][0]["external"], true);
    assert_eq!(value["emit"][0]["payload"]["ok"], true);
}

#[test]
fn await_rejects_internal_emitters_in_the_same_group() {
    use ostinato_domain::AwaitSpec;
    let mut steps = BTreeMap::new();
    let mut emitter = step("emit");
    emitter.emit = vec![ostinato_domain::EmitSpec {
        name: "ready".to_owned(),
        external: false,
        payload: None,
    }];
    let mut waiter = step("wait");
    waiter.kind = StepKind::Await;
    waiter.await_spec = Some(AwaitSpec {
        event: "ready".to_owned(),
        source: None,
        timeout_ms: Some(100),
    });
    steps.insert("emit".to_owned(), emitter);
    steps.insert("wait".to_owned(), waiter);
    let spec = WorkflowSpec {
        name: "same-group".to_owned(),
        max_parallel: 2,
        steps,
    };
    let error = build_plan(&spec).expect_err("same-group await must fail");
    assert!(matches!(error, DomainError::AwaitEmitterNotEarlier { .. }));
}

#[test]
fn await_accepts_an_emitter_in_a_strictly_earlier_group() {
    use ostinato_domain::AwaitSpec;
    let mut steps = BTreeMap::new();
    let mut emitter = step("emit");
    emitter.emit = vec![ostinato_domain::EmitSpec {
        name: "ready".to_owned(),
        external: false,
        payload: Some(json!({"value": 1})),
    }];
    let mut waiter = step("wait");
    waiter.kind = StepKind::Await;
    waiter.needs = vec!["emit".to_owned()];
    waiter.await_spec = Some(AwaitSpec {
        event: "ready".to_owned(),
        source: None,
        timeout_ms: Some(100),
    });
    steps.insert("emit".to_owned(), emitter);
    steps.insert("wait".to_owned(), waiter);
    let spec = WorkflowSpec {
        name: "earlier-group".to_owned(),
        max_parallel: 2,
        steps,
    };
    let plan = build_plan(&spec).expect("earlier emitter is valid");
    assert!(plan.nodes[0].group < plan.nodes[1].group);
}

#[test]
fn await_rejects_retry_and_missing_spec() {
    use ostinato_domain::AwaitSpec;
    let mut specs = BTreeMap::new();
    let mut waiter = step("wait");
    waiter.kind = StepKind::Await;
    waiter.await_spec = Some(AwaitSpec {
        event: "ready".to_owned(),
        source: None,
        timeout_ms: Some(100),
    });
    waiter.retry = RetryPolicy {
        max_retries: 1,
        ..RetryPolicy::default()
    };
    specs.insert("wait".to_owned(), waiter);
    let spec = WorkflowSpec {
        name: "retry".to_owned(),
        max_parallel: 1,
        steps: specs,
    };
    let error = build_plan(&spec).expect_err("await retry must fail");
    assert!(matches!(error, DomainError::AwaitRetryNotAllowed(_)));

    let mut specs = BTreeMap::new();
    let mut missing = step("wait");
    missing.kind = StepKind::Await;
    specs.insert("wait".to_owned(), missing);
    let spec = WorkflowSpec {
        name: "missing".to_owned(),
        max_parallel: 1,
        steps: specs,
    };
    let error = build_plan(&spec).expect_err("await spec must be required");
    assert!(matches!(error, DomainError::AwaitRequiresSpec(_)));
}

#[test]
fn event_names_and_payloads_enforce_public_limits() {
    use ostinato_domain::{
        MAX_EVENT_NAME_LEN, MAX_EVENT_PAYLOAD_BYTES, MAX_EVENT_PAYLOAD_DEPTH,
        validate_workflow_event,
    };
    assert!(validate_workflow_event("deploy.prod:web-1_ok", &json!({"a": 1})).is_ok());
    assert!(matches!(
        validate_workflow_event("bad name", &json!(null)),
        Err(DomainError::InvalidEventName)
    ));
    assert!(matches!(
        validate_workflow_event(&"a".repeat(MAX_EVENT_NAME_LEN + 1), &json!(null)),
        Err(DomainError::InvalidEventName)
    ));
    let mut deep = json!(null);
    for _ in 0..MAX_EVENT_PAYLOAD_DEPTH {
        deep = json!([deep]);
    }
    assert!(matches!(
        validate_workflow_event("deep", &deep),
        Err(DomainError::EventPayloadTooDeep { .. })
    ));
    let large = "x".repeat(MAX_EVENT_PAYLOAD_BYTES + 1);
    assert!(matches!(
        validate_workflow_event("large", &json!(large)),
        Err(DomainError::EventPayloadTooLarge { .. })
    ));
}
