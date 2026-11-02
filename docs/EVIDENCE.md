# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. This file is generated from `evidence/*.txt`; absolute temp paths are normalized but command output, hashes, UUIDs and exit codes are unchanged.

## Environment

Toolchain and pinning used for this evidence.

````text
ostinato 0.1.1
cue: cue version v0.17.1
rust: rustc 1.97.1 (8bab26f4f 2026-07-14) (Homebrew)
cargo: cargo 1.97.1 (c980f4866 2026-06-30) (Homebrew)
platform: Darwin 27.0.0 arm64
````

## Quality gates

Formatting, clippy with `-D warnings`, and the full test suite.

````text
### cargo fmt --all -- --check
exit=0

### cargo clippy --all-targets --all-features -- -D warnings
    Checking ostinato-domain v0.1.1 (./crates/ostinato-domain)
    Checking ostinato-lang v0.1.1 (./crates/ostinato-lang)
    Checking ostinato-application v0.1.1 (./crates/ostinato-application)
    Checking ostinato-adapters v0.1.1 (./crates/ostinato-adapters)
    Checking ostinato v0.1.1 (./crates/ostinato-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.32s
exit=0

### cargo test --all
   Compiling ostinato-domain v0.1.1 (./crates/ostinato-domain)
   Compiling ostinato-lang v0.1.1 (./crates/ostinato-lang)
   Compiling ostinato-application v0.1.1 (./crates/ostinato-application)
   Compiling ostinato-adapters v0.1.1 (./crates/ostinato-adapters)
   Compiling ostinato v0.1.1 (./crates/ostinato-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.26s
     Running unittests src/main.rs (target/debug/deps/ostinato-8713e0f5eff547c5)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/e2e.rs (target/debug/deps/e2e-5ed7ce006617feb0)

running 16 tests
test unknown_fields_are_rejected ... ok
test native_evaluator_supports_the_offline_subset ... ok
test truncation_blocks_resume_and_appends ... ok
test read_only_commands_do_not_require_cue ... ok
test bad_template_is_a_durable_failure ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
run=c89e27fc-ef33-40cf-93bd-a572703f5265 workflow=cancel status=cancelled durationMs=2015
STEP                     STATUS     VISITS ATTEMPTS
test state_tampering_is_detected_and_resume_refuses_it ... ok
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.33s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-c4bee0b51d7ff1fb)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-71aabbf53a081d69)

running 1 test
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-c832c150afe8db4e)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-dca9720420e0e9e4)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-afaa23c7eef32ae8)

running 6 tests
test new_event_helper_is_ergonomic ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test cycles_require_an_explicit_policy ... ok
test independent_steps_share_a_parallel_group ... ok
test cycles_expand_into_bounded_rounds ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-1bca59fe3f08ba0f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-c70b85f6f732466d)

running 10 tests
test renders_templates ... ok
test evaluates_conditions ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test unknown_workflow_fields_are_rejected ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test rejects_missing_required_parameter ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test compiles_workflow_and_resolves_parameters ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_adapters

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_application

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_domain

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_lang

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

exit=0
````

## Official CUE + module validation

A real CUE module with imports, `let`, a definition, comprehensions and builtins, evaluated by CUE v0.17.1 in package mode.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=official CUE (cue version v0.17.1))

exit=0
````

## Deterministic DAG

A normal two-branch workflow with dependency ordering and a typed parameter.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=7ab3321c-09df-45bc-8615-c439113a9c36 workflow=hello status=succeeded durationMs=10
STEP                     STATUS     VISITS ATTEMPTS
audit                    succeeded  1      1       
greet                    succeeded  1      1       
prepare                  succeeded  1      1       

exit=0
````

## Retries

A command fails twice and succeeds on the third attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/retry.cue

run=6965df84-2c31-458f-8014-85587239a0f8 workflow=retry status=succeeded durationMs=309
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

A review/refine cycle stops after the score crosses 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=10f02717-a12d-4bcf-a293-0088964813d0 workflow=cycle status=succeeded durationMs=51
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Step visits/attempts after the cycle.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status 10f02717-a12d-4bcf-a293-0088964813d0

run=10f02717-a12d-4bcf-a293-0088964813d0 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-02T16:19:27.576Z updated=2026-11-02T16:19:27.627Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

`cycle.completed`, checkpoint index and terminal events.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs 10f02717-a12d-4bcf-a293-0088964813d0

0 2026-11-02T16:19:27.576Z run.started step=- {"params":{},"workflow":"cycle"}
1 2026-11-02T16:19:27.586Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-02T16:19:27.590Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
3 2026-11-02T16:19:27.598Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
4 2026-11-02T16:19:27.604Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
5 2026-11-02T16:19:27.614Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
6 2026-11-02T16:19:27.618Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
7 2026-11-02T16:19:27.627Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
8 2026-11-02T16:19:27.627Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
9 2026-11-02T16:19:27.627Z run.succeeded step=- {"resumeIndex":11}

exit=0
````

## Trial log

One row per attempt with scores and durations.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials 10f02717-a12d-4bcf-a293-0088964813d0

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      8        
refine               0     0      succeeded 1.000      3        
review               0     0      succeeded 0.300      7        
refine               1     0      succeeded 1.000      5        
review               1     0      succeeded 0.600      8        
refine               2     0      succeeded 1.000      3        
review               2     0      succeeded 0.900      7        

exit=0
````

## Audit verification

Hash chain and state digest verify against the persisted heads.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify 10f02717-a12d-4bcf-a293-0088964813d0

OK run=10f02717-a12d-4bcf-a293-0088964813d0 events=10 head=42748e3d395b1c1be8e44f02c31936663602d3ad8602d555a90efbcf08b087fd

exit=0
````

## Metrics

Prometheus text output.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run 10f02717-a12d-4bcf-a293-0088964813d0 --format prometheus

# HELP ostinato_runs_total Runs started.
# TYPE ostinato_runs_total counter
ostinato_runs_total 1
ostinato_runs_succeeded_total 1
ostinato_runs_failed_total 0
ostinato_runs_running 0
ostinato_steps_total 3
ostinato_steps_failed_total 0
ostinato_attempts_total 7
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 5.86

exit=0
````

## Durability: failure

Step `b` fails; the checkpoint points at its group and `run.failed` is audited.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=e3ffa787-33f9-4efd-87b2-a54612b2a62e workflow=resume-evidence status=failed durationMs=9
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

After the external condition changes, `resume` finishes the run.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume e3ffa787-33f9-4efd-87b2-a54612b2a62e

run=e3ffa787-33f9-4efd-87b2-a54612b2a62e workflow=resume-evidence status=succeeded durationMs=4
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

`a` has exactly one trial; only `b` is retried.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials e3ffa787-33f9-4efd-87b2-a54612b2a62e

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      3        
b                    0     0      failed   0.000      4        command exited with 7: 
b                    0     0      succeeded 1.000      3        

exit=0
````

## Agentic workflow

An agent step, an improve step and a deterministic report step.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=e60ead12-6948-4f60-b691-e1b0408dbd74 workflow=agentic status=succeeded durationMs=28
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Two proposals, each re-evaluating CUE with patched params and producing a child run.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve e60ead12-6948-4f60-b691-e1b0408dbd74 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=e60ead12-6948-4f60-b691-e1b0408dbd74 diagnosis=The plan is sound; keep the scope small and reversible.
{
  "changes": [
    "no structural change"
  ],
  "confidence": 0.9,
  "diagnosis": "The plan is sound; keep the scope small and reversible.",
  "next_prompt": "Re-run with the improved goal and compare scores.",
  "params_patch": {
    "goal": "ship the improved release"
  },
  "score": 0.93
}
child run 9eb6d7cf-6837-4518-a827-26826d6c0cc0 status=succeeded parent=e60ead12-6948-4f60-b691-e1b0408dbd74
improvement #4 run=9eb6d7cf-6837-4518-a827-26826d6c0cc0 diagnosis=The plan is sound; keep the scope small and reversible.
{
  "changes": [
    "no structural change"
  ],
  "confidence": 0.9,
  "diagnosis": "The plan is sound; keep the scope small and reversible.",
  "next_prompt": "Re-run with the improved goal and compare scores.",
  "params_patch": {
    "goal": "ship the improved release"
  },
  "score": 0.93
}
child run 8b94af9b-d9df-4f54-a1f8-3395b9230b85 status=succeeded parent=9eb6d7cf-6837-4518-a827-26826d6c0cc0

exit=0
````

## Child runs

Run list showing the parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
8b94af9b-d9df-4f54-a1f8-3395b9230b85   agentic          succeeded  3        2026-11-02T16:19:27.920Z
9eb6d7cf-6837-4518-a827-26826d6c0cc0   agentic          succeeded  3        2026-11-02T16:19:27.865Z
e60ead12-6948-4f60-b691-e1b0408dbd74   agentic          succeeded  3        2026-11-02T16:19:27.780Z

exit=0
````

## Tamper detection

Mutating an audit payload makes verification fail.

````text
AUDIT FAILED run=e60ead12-6948-4f60-b691-e1b0408dbd74: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````
