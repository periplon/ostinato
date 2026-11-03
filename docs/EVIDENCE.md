# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.4.13
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
    Checking ostinato-domain v0.4.13 (./crates/ostinato-domain)
    Checking ostinato-lang v0.4.13 (./crates/ostinato-lang)
    Checking ostinato-application v0.4.13 (./crates/ostinato-application)
    Checking ostinato-adapters v0.4.13 (./crates/ostinato-adapters)
    Checking ostinato v0.4.13 (./crates/ostinato-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.20s
exit=0

### cargo test --all
   Compiling ostinato-domain v0.4.13 (./crates/ostinato-domain)
   Compiling ostinato-lang v0.4.13 (./crates/ostinato-lang)
   Compiling ostinato-application v0.4.13 (./crates/ostinato-application)
   Compiling ostinato-adapters v0.4.13 (./crates/ostinato-adapters)
   Compiling ostinato v0.4.13 (./crates/ostinato-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.77s
     Running unittests src/main.rs (target/debug/deps/ostinato-b76441eb5c6c3c16)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (target/debug/deps/conformance-dcaf0b6137711418)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.46s

     Running tests/e2e.rs (target/debug/deps/e2e-6088b347341d9566)

running 24 tests
test non_struct_params_are_rejected ... ok
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test package_underscore_files_are_file_local ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test native_evaluator_supports_the_offline_subset ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test unknown_fields_are_rejected ... ok
test read_only_commands_do_not_require_cue ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test bad_template_is_a_durable_failure ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test truncation_blocks_resume_and_appends ... ok
test official_cue_runs_and_audit_verifies ... ok
test resume_skips_completed_steps_and_finishes ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
run=65444004-a85b-42d4-a9c2-491c7efb1616 workflow=cancel status=cancelled durationMs=4034
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.12s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-53a32fc89a2a6111)

running 1 test
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-9556b55dde5c7498)

running 3 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-364431cfe8352ba8)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-57a28f2b3e160c07)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-444bd22866fe7d89)

running 6 tests
test retry_backoff_is_capped_and_deterministic ... ok
test new_event_helper_is_ergonomic ... ok
test cycles_require_an_explicit_policy ... ok
test independent_steps_share_a_parallel_group ... ok
test cycles_expand_into_bounded_rounds ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-3ac52d316692cba6)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-512b01af41279066)

running 119 tests
test acyclic_let_chains_are_accepted ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test arithmetic_uses_a_disjunction_default ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test aliases_bind_field_values ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test composed_definitions_keep_referenced_closedness ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test definition_disjunctions_keep_alternatives ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test compiles_workflow_and_resolves_parameters ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test derived_definitions_keep_referenced_closedness ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test comprehension_clause_sequences_are_supported ... ok
test disjunction_defaults_narrow_and_export ... ok
test duplicate_disjunction_defaults_collapse ... ok
test evaluates_conditions ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test forward_let_references_are_lazy ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test closed_definition_intersections_check_every_operand ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test invalid_aliases_are_rejected ... ok
test int_and_float_do_not_unify ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test let_cycles_are_rejected ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test missing_field_separators_are_rejected ... ok
test list_comprehensions_support_struct_bodies ... ok
test list_comprehensions_are_supported ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test multi_expression_definitions_close_after_composition ... ok
test named_definition_flattens_optional_field_materialization ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_definitions_are_selectable ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test open_list_element_types_are_enforced ... ok
test nested_patterns_constrain_nested_fields ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test optional_definition_fields_are_not_materialized ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test non_struct_definition_values_close_nested_structs ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_nested_definitions_are_not_selectable ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test optional_value_conflicts_are_still_deferred ... ok
test optional_values_get_patterns_before_closing ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test pattern_values_can_be_disjunctions ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test patterns_apply_to_declared_fields ... ok
test parenthesized_params_keep_overrides ... ok
test quoted_hash_labels_are_regular_fields ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test renders_templates ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test quoted_parameter_names_use_identifier_values ... ok
test rejects_missing_required_parameter ... ok
test retry_int_shorthand_respects_the_cap ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test step_level_unknown_fields_are_rejected ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test top_in_disjunctions_is_preserved ... ok
test string_index_addresses_regular_fields_only ... ok
test underscore_alias_is_rejected ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test unknown_workflow_fields_are_rejected ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test unicode_string_builtins_match_go_semantics ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test undemanded_let_cycles_are_rejected_statically ... ok
test unresolved_disjunctions_are_rejected_in_boolean_contexts ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok

test result: ok. 119 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

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

Rust vs CUE: quoted-label semantics (export-only, distinct from definitions, alias/index/selector rules, imports, comprehensions and generated optionals), field aliases and let/required-field cycle tracking (definitions, nested, dynamic, split files, inner let shadowing, dead-body gating, import/comprehension conflicts), list-comprehension struct and list bodies, closed/open/nested definitions, composed structs, patterns, metadata-aware disjunctions, deferred optionals, referenced admissions, disjunctions/defaults, parens, comprehensions, let/builtins, params, scope, hidden files, open lists, versioned/nested modules.

````text
### ./scripts/conformance.sh

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running tests/conformance.rs (target/debug/deps/conformance-dcaf0b6137711418)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.24s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=ecc61ba6-cbb6-4318-a5f6-e454d803a1bf workflow=hello status=succeeded durationMs=12
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

run=f2e9b075-1219-4421-a228-5990d1ab0b63 workflow=retry status=succeeded durationMs=322
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=b731546e-9744-472e-bdc1-645856489621 workflow=cycle status=succeeded durationMs=93
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Visits/attempts.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status b731546e-9744-472e-bdc1-645856489621

run=b731546e-9744-472e-bdc1-645856489621 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-03T01:19:00.348Z updated=2026-11-03T01:19:00.443Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs b731546e-9744-472e-bdc1-645856489621

0 2026-11-03T01:19:00.348Z run.started step=- {"params":{},"stateHash":"a90a55f77a374d6b6bb941201f6c71ddf0591fe74deacf8d3b1ebc9e5ddf2423","workflow":"cycle"}
1 2026-11-03T01:19:00.368Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-03T01:19:00.368Z state.committed step=- {"resumeIndex":1,"stateHash":"45c178124b49006c62b57096d8f276349f80c0130429ccad51790f757aa7c072"}
3 2026-11-03T01:19:00.378Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
4 2026-11-03T01:19:00.378Z state.committed step=- {"resumeIndex":2,"stateHash":"b3f61f97ce8fd8f448d206ad0bb4929834dd80a499e79c0dfa43f0719e80bdcf"}
5 2026-11-03T01:19:00.394Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
6 2026-11-03T01:19:00.394Z state.committed step=- {"resumeIndex":3,"stateHash":"424254928ad2790338ce7e8ee5ee96aba1bdcb3d0a37a693fe1b2614d5a35c3e"}
7 2026-11-03T01:19:00.403Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
8 2026-11-03T01:19:00.403Z state.committed step=- {"resumeIndex":4,"stateHash":"db9bc3dc2700ca875d4d905abeed329e13580a64255cfec75c9a94c2027cd309"}
9 2026-11-03T01:19:00.418Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
10 2026-11-03T01:19:00.418Z state.committed step=- {"resumeIndex":5,"stateHash":"3fac015bd38e6e64f6077e3b36469c70459db04f5f868e967603b26d4984ea76"}
11 2026-11-03T01:19:00.427Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
12 2026-11-03T01:19:00.427Z state.committed step=- {"resumeIndex":6,"stateHash":"babdea534dac4ee2586b9935fea4615f48ee838a7a27aeb26c3463ce72ced152"}
13 2026-11-03T01:19:00.441Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
14 2026-11-03T01:19:00.441Z state.committed step=- {"resumeIndex":7,"stateHash":"575e25bb558c08b854e9cbc400c5eb6ae16859d59c06308fed2f73d9669dbc18"}
15 2026-11-03T01:19:00.442Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
16 2026-11-03T01:19:00.442Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}
17 2026-11-03T01:19:00.443Z run.succeeded step=- {"resumeIndex":11}
18 2026-11-03T01:19:00.443Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}

exit=0
````

## Trial log

One row per attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials b731546e-9744-472e-bdc1-645856489621

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      17       
refine               0     0      succeeded 1.000      8        
review               0     0      succeeded 0.300      15       
refine               1     0      succeeded 1.000      8        
review               1     0      succeeded 0.600      14       
refine               2     0      succeeded 1.000      7        
review               2     0      succeeded 0.900      13       

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify b731546e-9744-472e-bdc1-645856489621

OK run=b731546e-9744-472e-bdc1-645856489621 events=19 head=31049ead18dace82220e23e1def8a638275c87d9e3bc5a453ae53128f4ef4c1d

exit=0
````

## Metrics

Prometheus text.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run b731546e-9744-472e-bdc1-645856489621 --format prometheus

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
ostinato_attempt_duration_ms_avg 11.71

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=ba7fe6e9-de2a-4ebc-9dde-fed45fcdb923 workflow=resume-evidence status=failed durationMs=13
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume ba7fe6e9-de2a-4ebc-9dde-fed45fcdb923

run=ba7fe6e9-de2a-4ebc-9dde-fed45fcdb923 workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials ba7fe6e9-de2a-4ebc-9dde-fed45fcdb923

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      6        
b                    0     0      failed   0.000      5        command exited with 7: 
b                    0     0      succeeded 1.000      5        

exit=0
````

## Agentic workflow

agent + improve + report.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f workflow=agentic status=succeeded durationMs=38
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 34e07c49-e998-468e-a9af-57d43e22b4ee status=succeeded parent=bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f
improvement #4 run=34e07c49-e998-468e-a9af-57d43e22b4ee diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 422bfcae-36f5-4be0-957c-d39a611d6232 status=succeeded parent=34e07c49-e998-468e-a9af-57d43e22b4ee

exit=0
````

## Child runs

Parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
422bfcae-36f5-4be0-957c-d39a611d6232   agentic          succeeded  3        2026-11-03T01:19:00.708Z
34e07c49-e998-468e-a9af-57d43e22b4ee   agentic          succeeded  3        2026-11-03T01:19:00.655Z
bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f   agentic          succeeded  3        2026-11-03T01:19:00.573Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=bd1acc90-0f47-48d2-86c3-bee9e2e7bf2f: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````
