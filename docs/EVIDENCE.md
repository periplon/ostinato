# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.5.0
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
    Checking ostinato-domain v0.5.0 (./crates/ostinato-domain)
    Checking ostinato-lang v0.5.0 (./crates/ostinato-lang)
    Checking ostinato-application v0.5.0 (./crates/ostinato-application)
    Checking ostinato-adapters v0.5.0 (./crates/ostinato-adapters)
    Checking ostinato v0.5.0 (./crates/ostinato-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.88s
exit=0

### cargo test --all
   Compiling ostinato-domain v0.5.0 (./crates/ostinato-domain)
   Compiling ostinato-lang v0.5.0 (./crates/ostinato-lang)
   Compiling ostinato-application v0.5.0 (./crates/ostinato-application)
   Compiling ostinato-adapters v0.5.0 (./crates/ostinato-adapters)
   Compiling ostinato v0.5.0 (./crates/ostinato-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.07s
     Running unittests src/main.rs (target/debug/deps/ostinato-102ae01fa8200397)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (target/debug/deps/conformance-f65b30207664fcb5)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.52s

     Running tests/e2e.rs (target/debug/deps/e2e-4b635f6d130586c2)

running 24 tests
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test package_underscore_files_are_file_local ... ok
test non_struct_params_are_rejected ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test native_evaluator_supports_the_offline_subset ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test unknown_fields_are_rejected ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test read_only_commands_do_not_require_cue ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test bad_template_is_a_durable_failure ... ok
test truncation_blocks_resume_and_appends ... ok
test resume_skips_completed_steps_and_finishes ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
run=7b802648-9bba-472e-90ea-b58c6c7b4e5d workflow=cancel status=cancelled durationMs=4029
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.17s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-6deb5e10d1b00355)

running 1 test
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-be4f425caf0e3711)

running 3 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-2debbbc8b126cd56)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-757871cfbb270dd2)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-7cea29a150d51a9e)

running 6 tests
test retry_backoff_is_capped_and_deterministic ... ok
test new_event_helper_is_ergonomic ... ok
test independent_steps_share_a_parallel_group ... ok
test cycles_require_an_explicit_policy ... ok
test cycles_expand_into_bounded_rounds ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-b7ac02ae50810acc)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-fdc542a68c0d1a46)

running 129 tests
test acyclic_let_chains_are_accepted ... ok
test aliases_bind_field_values ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test arithmetic_uses_a_disjunction_default ... ok
test composed_definitions_keep_referenced_closedness ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test bound_disjunctions_and_defaults_are_lattice_aware ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test alias_shadowing_keeps_dead_bodies_dead ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test compiles_workflow_and_resolves_parameters ... ok
test decimal_arithmetic_is_exact ... ok
test comprehension_clause_sequences_are_supported ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test definition_disjunctions_keep_alternatives ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test derived_definitions_keep_referenced_closedness ... ok
test disjunction_defaults_narrow_and_export ... ok
test bounds_reject_out_of_range_values ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test duplicate_disjunction_defaults_collapse ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test evaluates_conditions ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test closed_definition_intersections_check_every_operand ... ok
test bounds_narrow_concrete_values_and_types ... ok
test forward_let_references_are_lazy ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test generated_quoted_optionals_do_not_touch_materialized_definitions ... ok
test invalid_aliases_are_rejected ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test int_and_float_do_not_unify ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test let_cycles_are_rejected ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test list_comprehensions_support_struct_bodies ... ok
test missing_field_separators_are_rejected ... ok
test list_comprehensions_are_supported ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test multi_expression_definitions_close_after_composition ... ok
test named_definition_flattens_optional_field_materialization ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_definitions_are_selectable ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test nested_patterns_constrain_nested_fields ... ok
test open_list_element_types_are_enforced ... ok
test live_struct_sources_and_constant_conditions_keep_cycles ... ok
test optional_definition_fields_are_not_materialized ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test non_struct_definition_values_close_nested_structs ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test optional_nested_definitions_are_not_selectable ... ok
test optional_value_conflicts_are_still_deferred ... ok
test optional_values_get_patterns_before_closing ... ok
test nested_comprehension_bodies_shadow_in_static_liveness ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test pattern_values_can_be_disjunctions ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test patterns_apply_to_declared_fields ... ok
test parenthesized_params_keep_overrides ... ok
test quoted_hash_labels_are_regular_fields ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test renders_templates ... ok
test quoted_parameter_names_use_identifier_values ... ok
test rejects_missing_required_parameter ... ok
test retry_int_shorthand_respects_the_cap ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test step_level_unknown_fields_are_rejected ... ok
test underscore_alias_is_rejected ... ok
test string_index_addresses_regular_fields_only ... ok
test top_in_disjunctions_is_preserved ... ok
test unification_sources_are_live_when_either_side_is_live ... ok
test unicode_string_builtins_match_go_semantics ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test undemanded_let_cycles_are_rejected_statically ... ok
test struct_literal_shadowing_keeps_dead_bodies_dead ... ok
test unknown_workflow_fields_are_rejected ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test unresolved_disjunctions_are_rejected_in_boolean_contexts ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok

test result: ok. 129 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

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

Rust vs CUE: numeric/string bounds and exact decimals, quoted-label semantics, aliases, let/required-field cycle tracking, list comprehensions, closed/open/nested definitions, patterns, disjunctions/defaults, params, scope, hidden files, versioned/nested modules.

````text
### ./scripts/conformance.sh

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running tests/conformance.rs (target/debug/deps/conformance-f65b30207664fcb5)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.27s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=68c0118e-bcbd-4549-94ed-82983d9793c0 workflow=hello status=succeeded durationMs=13
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

run=18d88d01-f4ad-4c03-a922-bb6ae7aa930d workflow=retry status=succeeded durationMs=331
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=a2311325-675e-4116-9132-151a29f61530 workflow=cycle status=succeeded durationMs=98
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Visits/attempts.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status a2311325-675e-4116-9132-151a29f61530

run=a2311325-675e-4116-9132-151a29f61530 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-03T02:31:31.200Z updated=2026-11-03T02:31:31.299Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs a2311325-675e-4116-9132-151a29f61530

0 2026-11-03T02:31:31.200Z run.started step=- {"params":{},"stateHash":"a90a55f77a374d6b6bb941201f6c71ddf0591fe74deacf8d3b1ebc9e5ddf2423","workflow":"cycle"}
1 2026-11-03T02:31:31.224Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-03T02:31:31.224Z state.committed step=- {"resumeIndex":1,"stateHash":"45c178124b49006c62b57096d8f276349f80c0130429ccad51790f757aa7c072"}
3 2026-11-03T02:31:31.235Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
4 2026-11-03T02:31:31.235Z state.committed step=- {"resumeIndex":2,"stateHash":"b3f61f97ce8fd8f448d206ad0bb4929834dd80a499e79c0dfa43f0719e80bdcf"}
5 2026-11-03T02:31:31.251Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
6 2026-11-03T02:31:31.251Z state.committed step=- {"resumeIndex":3,"stateHash":"424254928ad2790338ce7e8ee5ee96aba1bdcb3d0a37a693fe1b2614d5a35c3e"}
7 2026-11-03T02:31:31.261Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
8 2026-11-03T02:31:31.261Z state.committed step=- {"resumeIndex":4,"stateHash":"db9bc3dc2700ca875d4d905abeed329e13580a64255cfec75c9a94c2027cd309"}
9 2026-11-03T02:31:31.275Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
10 2026-11-03T02:31:31.275Z state.committed step=- {"resumeIndex":5,"stateHash":"3fac015bd38e6e64f6077e3b36469c70459db04f5f868e967603b26d4984ea76"}
11 2026-11-03T02:31:31.284Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
12 2026-11-03T02:31:31.284Z state.committed step=- {"resumeIndex":6,"stateHash":"babdea534dac4ee2586b9935fea4615f48ee838a7a27aeb26c3463ce72ced152"}
13 2026-11-03T02:31:31.298Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
14 2026-11-03T02:31:31.298Z state.committed step=- {"resumeIndex":7,"stateHash":"575e25bb558c08b854e9cbc400c5eb6ae16859d59c06308fed2f73d9669dbc18"}
15 2026-11-03T02:31:31.299Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
16 2026-11-03T02:31:31.299Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}
17 2026-11-03T02:31:31.299Z run.succeeded step=- {"resumeIndex":11}
18 2026-11-03T02:31:31.299Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}

exit=0
````

## Trial log

One row per attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials a2311325-675e-4116-9132-151a29f61530

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      21       
refine               0     0      succeeded 1.000      9        
review               0     0      succeeded 0.300      15       
refine               1     0      succeeded 1.000      8        
review               1     0      succeeded 0.600      13       
refine               2     0      succeeded 1.000      7        
review               2     0      succeeded 0.900      13       

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify a2311325-675e-4116-9132-151a29f61530

OK run=a2311325-675e-4116-9132-151a29f61530 events=19 head=962eaacb3ba405eb7a0e5ecf870d9e84d4c8533741292e8b8bf224ed61c0a9fc

exit=0
````

## Metrics

Prometheus text.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run a2311325-675e-4116-9132-151a29f61530 --format prometheus

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
ostinato_attempt_duration_ms_avg 12.29

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=5fa04406-63e5-4462-b7dd-803da29a6b96 workflow=resume-evidence status=failed durationMs=12
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume 5fa04406-63e5-4462-b7dd-803da29a6b96

run=5fa04406-63e5-4462-b7dd-803da29a6b96 workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials 5fa04406-63e5-4462-b7dd-803da29a6b96

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      5        
b                    0     0      failed   0.000      4        command exited with 7: 
b                    0     0      succeeded 1.000      5        

exit=0
````

## Agentic workflow

agent + improve + report.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=2886f1f2-20c2-422a-bf0b-2696c9fb77c7 workflow=agentic status=succeeded durationMs=30
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve 2886f1f2-20c2-422a-bf0b-2696c9fb77c7 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=2886f1f2-20c2-422a-bf0b-2696c9fb77c7 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 1347a693-783d-434c-89dc-680543489ef9 status=succeeded parent=2886f1f2-20c2-422a-bf0b-2696c9fb77c7
improvement #4 run=1347a693-783d-434c-89dc-680543489ef9 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 52d38c94-a1f8-4cf9-a579-3fb86be98e07 status=succeeded parent=1347a693-783d-434c-89dc-680543489ef9

exit=0
````

## Child runs

Parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
52d38c94-a1f8-4cf9-a579-3fb86be98e07   agentic          succeeded  3        2026-11-03T02:31:31.535Z
1347a693-783d-434c-89dc-680543489ef9   agentic          succeeded  3        2026-11-03T02:31:31.491Z
2886f1f2-20c2-422a-bf0b-2696c9fb77c7   agentic          succeeded  3        2026-11-03T02:31:31.424Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=2886f1f2-20c2-422a-bf0b-2696c9fb77c7: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````
