#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ostinato_domain::{
    AuditEvent, CycleSpec, DomainError, NewEvent, RetryPolicy, StepKind, StepSpec, WorkflowSpec,
    audit, build_plan,
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
