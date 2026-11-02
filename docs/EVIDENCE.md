# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.2.1
cue: cue version v0.17.1
rust: rustc 1.97.1 (8bab26f4f 2026-07-14) (Homebrew)
cargo: cargo 1.97.1 (c980f4866 2026-06-30) (Homebrew)
platform: Darwin 27.0.0 arm64
````

## Quality gates

fmt, clippy, full tests including conformance.

````text
### cargo fmt --all -- --check
exit=0

### cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
exit=0

### cargo test --all
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/main.rs (target/debug/deps/ostinato-561ad1c026d9ebf7)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (target/debug/deps/conformance-e606433b38bd1b73)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.64s

     Running tests/e2e.rs (target/debug/deps/e2e-168341b1cebc5c85)

running 23 tests
test non_struct_params_are_rejected ... ok
test package_underscore_files_are_file_local ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test native_evaluator_supports_the_offline_subset ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test unknown_fields_are_rejected ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test audit_verification_detects_tampering ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test read_only_commands_do_not_require_cue ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test bad_template_is_a_durable_failure ... ok
test truncation_blocks_resume_and_appends ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
run=dd0fe34d-53a7-43c4-9712-f6ee2198ca44 workflow=cancel status=cancelled durationMs=4025
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.09s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-0992025eb8e69d3b)

running 1 test
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-0b8db18d2e6b7f93)

running 3 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-dd9275ee76c8bfc0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-420a754fb6845cb5)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-339ec1807ccfd19e)

running 6 tests
test new_event_helper_is_ergonomic ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test cycles_require_an_explicit_policy ... ok
test cycles_expand_into_bounded_rounds ... ok
test independent_steps_share_a_parallel_group ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-3062d7f65c0e6e49)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-b130b0a08d7fd37c)

running 33 tests
test evaluates_conditions ... ok
test missing_field_separators_are_rejected ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test definitions_include_comprehension_and_conditional_fields ... ok
test forward_let_references_are_lazy ... ok
test open_list_element_types_are_enforced ... ok
test list_comprehensions_are_supported ... ok
test closed_definitions_reject_unknown_fields ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test open_and_closed_intersection_is_closed ... ok
test closed_definitions_allow_optional_fields_and_open_ellipsis ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test comprehension_clause_sequences_are_supported ... ok
test closed_closed_intersection_rejects_both_directions_and_keeps_optionality ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test renders_templates ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test compiles_workflow_and_resolves_parameters ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test retry_int_shorthand_respects_the_cap ... ok
test rejects_missing_required_parameter ... ok
test split_definitions_merge_and_optional_values_are_not_materialized ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test step_level_unknown_fields_are_rejected ... ok
test unknown_workflow_fields_are_rejected ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test unicode_string_builtins_match_go_semantics ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok

test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

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

## Rust default evaluator

Module workflow via Rust.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=rust (CUE workflow semantics))

exit=0
````

## CUE oracle

Same workflow via CUE v0.17.1.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --evaluator cue --cue cue validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=official CUE (cue version v0.17.1))

exit=0
````

## Conformance corpus

Rust vs CUE on the corpus: defaults, unification, closedness/optional/open, comprehensions, list comprehensions, let/builtins, params, scope, hidden files, open lists, versioned modules.

````text
### ./scripts/conformance.sh

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running tests/conformance.rs (target/debug/deps/conformance-e606433b38bd1b73)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.65s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=60924024-bbe7-452f-88c2-4cf23d700275 workflow=hello status=succeeded durationMs=13
STEP                     STATUS     VISITS ATTEMPTS
audit                    succeeded  1      1       
greet                    succeeded  1      1       
prepare                  succeeded  1      1       

exit=0
````

## Retries

Fail twice, succeed on third attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/retry.cue

run=ef859f8f-f75d-4e25-8aaa-81635228a0fc workflow=retry status=succeeded durationMs=319
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=ddef0d03-880d-44aa-bb9a-c710259868f9 workflow=cycle status=succeeded durationMs=61
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Visits/attempts.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status ddef0d03-880d-44aa-bb9a-c710259868f9

run=ddef0d03-880d-44aa-bb9a-c710259868f9 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-02T18:10:15.886Z updated=2026-11-02T18:10:15.948Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs ddef0d03-880d-44aa-bb9a-c710259868f9

0 2026-11-02T18:10:15.886Z run.started step=- {"params":{},"stateHash":"a90a55f77a374d6b6bb941201f6c71ddf0591fe74deacf8d3b1ebc9e5ddf2423","workflow":"cycle"}
1 2026-11-02T18:10:15.899Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-02T18:10:15.899Z state.committed step=- {"resumeIndex":1,"stateHash":"45c178124b49006c62b57096d8f276349f80c0130429ccad51790f757aa7c072"}
3 2026-11-02T18:10:15.906Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
4 2026-11-02T18:10:15.906Z state.committed step=- {"resumeIndex":2,"stateHash":"b3f61f97ce8fd8f448d206ad0bb4929834dd80a499e79c0dfa43f0719e80bdcf"}
5 2026-11-02T18:10:15.915Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
6 2026-11-02T18:10:15.915Z state.committed step=- {"resumeIndex":3,"stateHash":"424254928ad2790338ce7e8ee5ee96aba1bdcb3d0a37a693fe1b2614d5a35c3e"}
7 2026-11-02T18:10:15.922Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
8 2026-11-02T18:10:15.922Z state.committed step=- {"resumeIndex":4,"stateHash":"db9bc3dc2700ca875d4d905abeed329e13580a64255cfec75c9a94c2027cd309"}
9 2026-11-02T18:10:15.931Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
10 2026-11-02T18:10:15.931Z state.committed step=- {"resumeIndex":5,"stateHash":"3fac015bd38e6e64f6077e3b36469c70459db04f5f868e967603b26d4984ea76"}
11 2026-11-02T18:10:15.937Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
12 2026-11-02T18:10:15.937Z state.committed step=- {"resumeIndex":6,"stateHash":"babdea534dac4ee2586b9935fea4615f48ee838a7a27aeb26c3463ce72ced152"}
13 2026-11-02T18:10:15.947Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
14 2026-11-02T18:10:15.947Z state.committed step=- {"resumeIndex":7,"stateHash":"575e25bb558c08b854e9cbc400c5eb6ae16859d59c06308fed2f73d9669dbc18"}
15 2026-11-02T18:10:15.948Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
16 2026-11-02T18:10:15.948Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}
17 2026-11-02T18:10:15.948Z run.succeeded step=- {"resumeIndex":11}
18 2026-11-02T18:10:15.948Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}

exit=0
````

## Trial log

One row per attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials ddef0d03-880d-44aa-bb9a-c710259868f9

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      12       
refine               0     0      succeeded 1.000      5        
review               0     0      succeeded 0.300      8        
refine               1     0      succeeded 1.000      5        
review               1     0      succeeded 0.600      9        
refine               2     0      succeeded 1.000      5        
review               2     0      succeeded 0.900      9        

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify ddef0d03-880d-44aa-bb9a-c710259868f9

OK run=ddef0d03-880d-44aa-bb9a-c710259868f9 events=19 head=b8482db6d94d3b0bb8da109011cc2f78116b62c71bef54609d32a7756c9fcaba

exit=0
````

## Metrics

Prometheus text.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run ddef0d03-880d-44aa-bb9a-c710259868f9 --format prometheus

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
ostinato_attempt_duration_ms_avg 7.57

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=1120f37b-6d1d-459d-8e75-bd1683a2b4bf workflow=resume-evidence status=failed durationMs=12
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume 1120f37b-6d1d-459d-8e75-bd1683a2b4bf

run=1120f37b-6d1d-459d-8e75-bd1683a2b4bf workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials 1120f37b-6d1d-459d-8e75-bd1683a2b4bf

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      5        
b                    0     0      failed   0.000      5        command exited with 7: 
b                    0     0      succeeded 1.000      5        

exit=0
````

## Agentic workflow

agent + improve + report.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=52c9d94d-df32-4c58-8ab2-541976d58e4b workflow=agentic status=succeeded durationMs=35
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve 52c9d94d-df32-4c58-8ab2-541976d58e4b --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=52c9d94d-df32-4c58-8ab2-541976d58e4b diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 314e2f2c-bdaa-4230-9243-3ab430999eb3 status=succeeded parent=52c9d94d-df32-4c58-8ab2-541976d58e4b
improvement #4 run=314e2f2c-bdaa-4230-9243-3ab430999eb3 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run aa0e1fc1-fc46-4e35-b896-27daf96584f3 status=succeeded parent=314e2f2c-bdaa-4230-9243-3ab430999eb3

exit=0
````

## Child runs

Parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
aa0e1fc1-fc46-4e35-b896-27daf96584f3   agentic          succeeded  3        2026-11-02T18:10:16.197Z
314e2f2c-bdaa-4230-9243-3ab430999eb3   agentic          succeeded  3        2026-11-02T18:10:16.147Z
52c9d94d-df32-4c58-8ab2-541976d58e4b   agentic          succeeded  3        2026-11-02T18:10:16.072Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=52c9d94d-df32-4c58-8ab2-541976d58e4b: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````
