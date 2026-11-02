# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. This file is generated from `evidence/*.txt`; absolute temp paths are normalized but command output, hashes, UUIDs and exit codes are unchanged.

## Environment

Toolchain and pinning used for this evidence.

````text
ostinato 0.1.2
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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
exit=0

### cargo test --all
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/main.rs (target/debug/deps/ostinato-27b7125b8ed70c51)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/e2e.rs (target/debug/deps/e2e-29e727b2704bd24e)

running 21 tests
test package_less_cue_file_is_loaded_by_filename ... ok
test native_evaluator_supports_the_offline_subset ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test unknown_fields_are_rejected ... ok
test read_only_commands_do_not_require_cue ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test bad_template_is_a_durable_failure ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test truncation_blocks_resume_and_appends ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test resume_skips_completed_steps_and_finishes ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
run=b70a688b-b673-46d3-9f24-f45417432c45 workflow=cancel status=cancelled durationMs=2025
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.25s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-8d72d5dee34e9c8a)

running 1 test
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-312080a9a09668f1)

running 3 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-1e74540229a00dfd)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-76574cf0b9346113)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-9edbc9595035500b)

running 6 tests
test retry_backoff_is_capped_and_deterministic ... ok
test cycles_require_an_explicit_policy ... ok
test cycles_expand_into_bounded_rounds ... ok
test new_event_helper_is_ergonomic ... ok
test independent_steps_share_a_parallel_group ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-665f4694b34662a1)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-d2276f6e7cb5e44e)

running 14 tests
test evaluates_conditions ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test renders_templates ... ok
test step_level_unknown_fields_are_rejected ... ok
test retry_int_shorthand_respects_the_cap ... ok
test unknown_workflow_fields_are_rejected ... ok
test rejects_missing_required_parameter ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test compiles_workflow_and_resolves_parameters ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

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

run=27673937-0d1e-41ab-bc53-9252147b813d workflow=hello status=succeeded durationMs=12
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

run=3439cbde-2df7-4cab-acc8-3c7e913358b4 workflow=retry status=succeeded durationMs=311
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

A review/refine cycle stops after the score crosses 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=36aee0db-a5f1-453a-9694-4e60c67edf2f workflow=cycle status=succeeded durationMs=56
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Step visits/attempts after the cycle.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status 36aee0db-a5f1-453a-9694-4e60c67edf2f

run=36aee0db-a5f1-453a-9694-4e60c67edf2f workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-02T16:41:32.049Z updated=2026-11-02T16:41:32.106Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

`state.committed`, `cycle.completed`, checkpoint index and terminal events.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs 36aee0db-a5f1-453a-9694-4e60c67edf2f

0 2026-11-02T16:41:32.049Z run.started step=- {"params":{},"stateHash":"a90a55f77a374d6b6bb941201f6c71ddf0591fe74deacf8d3b1ebc9e5ddf2423","workflow":"cycle"}
1 2026-11-02T16:41:32.060Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-02T16:41:32.060Z state.committed step=- {"resumeIndex":1,"stateHash":"45c178124b49006c62b57096d8f276349f80c0130429ccad51790f757aa7c072"}
3 2026-11-02T16:41:32.065Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
4 2026-11-02T16:41:32.065Z state.committed step=- {"resumeIndex":2,"stateHash":"b3f61f97ce8fd8f448d206ad0bb4929834dd80a499e79c0dfa43f0719e80bdcf"}
5 2026-11-02T16:41:32.075Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
6 2026-11-02T16:41:32.075Z state.committed step=- {"resumeIndex":3,"stateHash":"424254928ad2790338ce7e8ee5ee96aba1bdcb3d0a37a693fe1b2614d5a35c3e"}
7 2026-11-02T16:41:32.080Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
8 2026-11-02T16:41:32.080Z state.committed step=- {"resumeIndex":4,"stateHash":"db9bc3dc2700ca875d4d905abeed329e13580a64255cfec75c9a94c2027cd309"}
9 2026-11-02T16:41:32.089Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
10 2026-11-02T16:41:32.089Z state.committed step=- {"resumeIndex":5,"stateHash":"3fac015bd38e6e64f6077e3b36469c70459db04f5f868e967603b26d4984ea76"}
11 2026-11-02T16:41:32.095Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
12 2026-11-02T16:41:32.095Z state.committed step=- {"resumeIndex":6,"stateHash":"babdea534dac4ee2586b9935fea4615f48ee838a7a27aeb26c3463ce72ced152"}
13 2026-11-02T16:41:32.105Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
14 2026-11-02T16:41:32.105Z state.committed step=- {"resumeIndex":7,"stateHash":"575e25bb558c08b854e9cbc400c5eb6ae16859d59c06308fed2f73d9669dbc18"}
15 2026-11-02T16:41:32.105Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
16 2026-11-02T16:41:32.105Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}
17 2026-11-02T16:41:32.106Z run.succeeded step=- {"resumeIndex":11}
18 2026-11-02T16:41:32.106Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}

exit=0
````

## Trial log

One row per attempt with scores and durations.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials 36aee0db-a5f1-453a-9694-4e60c67edf2f

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      9        
refine               0     0      succeeded 1.000      4        
review               0     0      succeeded 0.300      8        
refine               1     0      succeeded 1.000      5        
review               1     0      succeeded 0.600      8        
refine               2     0      succeeded 1.000      5        
review               2     0      succeeded 0.900      8        

exit=0
````

## Audit verification

Hash chain, state digest and `state.committed` anchor verify.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify 36aee0db-a5f1-453a-9694-4e60c67edf2f

OK run=36aee0db-a5f1-453a-9694-4e60c67edf2f events=19 head=a0dbca26c8149863090c8aedfea0238bbaded878355b9aa6635fd863d5a1c0ac

exit=0
````

## Metrics

Prometheus text output.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run 36aee0db-a5f1-453a-9694-4e60c67edf2f --format prometheus

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
ostinato_attempt_duration_ms_avg 6.71

exit=0
````

## Durability: failure

Step `b` fails; the checkpoint points at its group and `run.failed` is audited.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=cc1a8464-6a78-43dc-9043-d543f5f08fc8 workflow=resume-evidence status=failed durationMs=12
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

After the external condition changes, `resume` finishes the run.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume cc1a8464-6a78-43dc-9043-d543f5f08fc8

run=cc1a8464-6a78-43dc-9043-d543f5f08fc8 workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

`a` has exactly one trial; only `b` is retried.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials cc1a8464-6a78-43dc-9043-d543f5f08fc8

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      5        
b                    0     0      failed   0.000      5        command exited with 7: 
b                    0     0      succeeded 1.000      5        

exit=0
````

## Agentic workflow

An agent step, an improve step and a deterministic report step.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=4519c824-a4b4-44e4-b930-4d91d65df284 workflow=agentic status=succeeded durationMs=32
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Two proposals, each re-evaluating CUE with patched params and producing a child run.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve 4519c824-a4b4-44e4-b930-4d91d65df284 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=4519c824-a4b4-44e4-b930-4d91d65df284 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 8e8b7905-d9bd-4694-a0bc-c1853722d2ad status=succeeded parent=4519c824-a4b4-44e4-b930-4d91d65df284
improvement #4 run=8e8b7905-d9bd-4694-a0bc-c1853722d2ad diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 3e94486c-dd1f-4248-a017-843e67f5381e status=succeeded parent=8e8b7905-d9bd-4694-a0bc-c1853722d2ad

exit=0
````

## Child runs

Run list showing the parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
3e94486c-dd1f-4248-a017-843e67f5381e   agentic          succeeded  3        2026-11-02T16:41:32.406Z
8e8b7905-d9bd-4694-a0bc-c1853722d2ad   agentic          succeeded  3        2026-11-02T16:41:32.347Z
4519c824-a4b4-44e4-b930-4d91d65df284   agentic          succeeded  3        2026-11-02T16:41:32.260Z

exit=0
````

## Tamper detection

Mutating an audit payload makes verification fail.

````text
AUDIT FAILED run=4519c824-a4b4-44e4-b930-4d91d65df284: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````
