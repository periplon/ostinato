# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.17.1
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
     Running unittests src/main.rs (target/debug/deps/ostinato-c004507ae5a13b44)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (target/debug/deps/conformance-83e2b549b3c42212)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.84s

     Running tests/e2e.rs (target/debug/deps/e2e-f1b7776a5e723f54)

running 61 tests
test duplicate_and_nested_tags_follow_cue ... ok
test bare_open_list_needs_and_params_compile ... ok
test gateway_graph_dot_and_json_expose_routing_metadata ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test gateway_failure_is_recorded_and_keeps_resume_index_at_the_gateway_group ... ok
test gateway_audit_verifies_and_detects_tampered_gateway_output ... ok
test gateway_audit_detects_a_tampered_skipped_row ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test hidden_metadata_is_allowed_throughout_the_workflow_schema ... ok
test gateway_dead_path_propagates_and_records_skips ... ok
test bad_template_is_a_durable_failure ... ok
test let_and_alias_backed_params_tags_work ... ok
test gateway_or_runs_every_true_branch ... ok
test nested_definition_tags_inject_their_declaration ... ok
test gateway_and_fan_out_and_fan_in_wait_for_both_branches ... ok
test nested_tag_aliases_resolve_at_their_written_scope ... ok
test gateway_routes_on_continue_on_error_status ... ok
test non_struct_params_are_rejected ... ok
test nested_tags_work_through_definitions_and_reject_bad_types ... ok
test gateway_xor_default_branch_runs_when_no_condition_matches ... ok
test native_evaluator_supports_the_offline_subset ... ok
test nested_tags_cover_definitions_hidden_quoted_and_shorthand ... ok
test package_underscore_files_are_file_local ... ok
test nested_tags_follow_label_identity_depth_and_expression_bases ... ok
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test params_siblings_resolve_and_true_cycles_fail_cleanly ... ok
test recursive_definitions_do_not_overflow_tag_collection ... ok
test gateway_resume_after_split_does_not_re_evaluate_or_run_dead_branch ... ok
test leaf_tag_injection_preserves_lexical_scopes ... ok
test multiline_and_interpolated_bytes_match_both_evaluators ... ok
test selector_tags_cover_leaves_duplicates_and_shadowed_definitions ... ok
test selector_tags_follow_definitions_duplicates_and_scope ... ok
test nested_tags_inject_nested_parameters ... ok
test split_and_duplicate_definition_tags_work ... ok
test tag_attributes_drive_parameter_injection_types ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test cycles_stop_when_until_becomes_true ... ok
test read_only_commands_do_not_require_cue ... ok
test legacy_run_without_gateways_still_resumes_and_verifies ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test tag_identity_handles_shadowing_selectors_and_disjunctions ... ok
test tag_names_and_value_grammar_follow_cue ... ok
test gateway_xor_routes_one_branch_and_skips_the_other ... ok
test tag_number_grammar_and_parenthesized_params ... ok
test recursive_definitions_materialize_in_both_evaluators ... ok
test unknown_fields_are_rejected ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test tag_literal_grammar_and_strict_types ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test truncation_blocks_resume_and_appends ... ok
test resume_skips_completed_steps_and_finishes ... ok
test recursive_improvement_creates_child_runs ... ok
test exec_timeout_kills_the_process_group ... ok
test deep_recursive_values_do_not_exhaust_the_stack ... ok
run=23c96b00-fe60-4f96-b2f9-5a2bcf90ac26 workflow=cancel status=cancelled durationMs=4031
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok
test definition_backed_tags_work_in_both_evaluators ... ok

test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.11s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-0aa49c739fe27ffd)

running 1 test
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-4be9de845dc45405)

running 3 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-6b9e41257e09758c)

running 18 tests
test activation::tests::and_split_selects_all_branches ... ok
test activation::tests::and_join_requires_every_active_input ... ok
test activation::tests::legacy_multiple_needs_keep_and_semantics ... ok
test activation::tests::condition_evaluation_error_is_a_gateway_error ... ok
test activation::tests::missing_predecessor_is_invalid_state ... ok
test activation::tests::or_no_true_no_default_fails ... ok
test activation::tests::normal_step_with_dead_input_is_dead ... ok
test activation::tests::or_none_uses_default ... ok
test activation::tests::or_join_needs_one_active_input ... ok
test activation::tests::or_selects_every_true_branch ... ok
test activation::tests::split_gateway_branch_choice_controls_edge_state ... ok
test activation::tests::xor_join_one_active_is_active ... ok
test activation::tests::xor_join_two_active_is_ambiguous ... ok
test activation::tests::xor_join_zero_active_is_dead ... ok
test activation::tests::xor_no_true_no_default_fails ... ok
test activation::tests::xor_no_true_uses_default ... ok
test activation::tests::xor_two_true_is_ambiguous ... ok
test activation::tests::xor_unique_true_selects_one ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-a5a61ee791c6b8a0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-c6c39fb01979e92e)

running 17 tests
test branch_target_with_needs_is_rejected ... ok
test cycles_require_an_explicit_policy ... ok
test gateway_and_cycle_combination_is_rejected ... ok
test invalid_default_is_rejected ... ok
test join_with_branches_is_rejected ... ok
test independent_steps_share_a_parallel_group ... ok
test new_event_helper_is_ergonomic ... ok
test legacy_step_spec_deserializes_without_gateway_and_reserializes_identically ... ok
test implicit_join_with_gateways_is_rejected ... ok
test legacy_plan_shape_is_unchanged_without_gateways ... ok
test audit_chain_detects_tampering ... ok
test cycles_expand_into_bounded_rounds ... ok
test gateway_plan_orders_branches_and_joins ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test legacy_state_digest_is_stable_across_the_gateway_change ... ok
test split_and_join_together_is_rejected ... ok
test unknown_branch_target_is_rejected ... ok

test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-355858bafd7f1bd9)

running 3 tests
test gotime::tests::formats_durations ... ok
test gotime::tests::formats_go_layouts ... ok
test gotime::tests::parses_go_layouts ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-0590d681d4c889b7)

running 238 tests
test aliases_bind_field_values ... ok
test aliases_to_control_prefix_quoted_labels_resolve ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test acyclic_let_chains_are_accepted ... ok
test absolute_params_paths_do_not_shadow_into_nested_groups ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test arithmetic_uses_a_disjunction_default ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test attributes_parse_in_every_cue_position ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test absolute_params_references_inside_generated_groups_resolve ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test bare_open_lists_behave_as_empty_lists ... ok
test bytes_literals_use_go_escape_rules ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test bound_disjunctions_and_defaults_are_lattice_aware ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test closed_definitions_stay_closed_through_params ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test chained_dynamic_groups_resolve_sibling_values ... ok
test bounds_narrow_concrete_values_and_types ... ok
test arbitrary_precision_integer_literals_keep_exact_values ... ok
test closed_definitions_apply_to_nested_dynamic_and_let_params ... ok
test closedness_through_let_mediated_definition_bodies ... ok
test closed_definitions_are_checked_inside_referenced_bodies ... ok
test comprehension_clause_sequences_are_supported ... ok
test bytes_interpolation_formats_scalars_and_rejects_incomplete ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test bounds_reject_out_of_range_values ... ok
test composed_definitions_keep_referenced_closedness ... ok
test compiles_workflow_and_resolves_parameters ... ok
test alias_shadowing_keeps_dead_bodies_dead ... ok
test comprehension_labels_use_displayed_names_for_control_prefixes ... ok
test big_integer_bounds_and_builtins_match_cue ... ok
test cross_conditional_params_groups_share_materialized_values ... ok
test decimal_arithmetic_is_exact ... ok
test control_prefix_labels_match_patterns ... ok
test branch_condition_scope_is_enforced_at_compile_time ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test closed_definition_intersections_check_every_operand ... ok
test comprehension_generated_definitions_respect_caches_and_shadowing ... ok
test comprehension_generated_definitions_are_selectable_and_hidden ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test comprehension_generated_declarations_merge_like_cue ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test definition_selectors_and_quoted_hash_fields_in_params_paths ... ok
test derived_definitions_keep_referenced_closedness ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test duplicate_disjunction_defaults_collapse ... ok
test disjunction_defaults_narrow_and_export ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test evaluates_conditions ... ok
test definition_disjunctions_keep_alternatives ... ok
test dynamic_index_then_selector_resolves_params_paths ... ok
test division_by_zero_is_an_error_not_a_panic ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test bounds_validate_operands_types_and_intervals ... ok
test definition_backed_params_see_overrides_in_dependents ... ok
test dynamic_intermediate_and_repeated_segments_resolve ... ok
test compiles_every_split_and_join_mode ... ok
test escaped_patterns_and_quoted_labels_decode_correctly ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test forward_let_references_are_lazy ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test generated_hidden_optionals_do_not_hide_visible_sentinel_labels ... ok
test generated_hidden_optionals_in_definitions_are_optional ... ok
test gateway_condition_scope_accepts_transitive_predecessors_and_ignores_string_literals ... ok
test gateway_definition_backed_branches_compile_identically ... ok
test gateway_compiles_identically_through_value_and_json ... ok
test escaped_slash_case_mapping_and_decimal_ranges ... ok
test gateway_conditions_and_step_status_are_accepted ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test gateway_cycles_and_implicit_joins_are_rejected_at_compile_time ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test generated_hidden_params_are_not_reachable_through_quoted_selectors ... ok
test generated_root_optionals_do_not_hide_definitions ... ok
test equality_requires_concrete_operands ... ok
test hidden_fields_are_invisible_to_value_operations ... ok
test generated_quoted_optionals_do_not_touch_materialized_definitions ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test generated_quoted_hash_and_control_labels_are_selectable ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test imported_packages_drop_package_hidden_fields ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test imported_hidden_fields_do_not_leak_into_the_importer ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test hidden_fields_are_distinct_from_quoted_underscore_fields ... ok
test identifier_selectors_prefer_hidden_over_quoted_twins ... ok
test generated_params_fields_respect_hidden_quoting_namespaces ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test invalid_aliases_are_rejected ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test int_and_float_do_not_unify ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test hidden_fields_are_evaluated_selected_and_conflict_checked ... ok
test let_cycles_are_rejected ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test list_comprehensions_support_struct_bodies ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test missing_field_separators_are_rejected ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test list_comprehensions_are_supported ... ok
test materialized_sentinel_fields_stay_required_and_conflict_checked ... ok
test multiline_bytes_escaped_backslash_before_paren_is_literal ... ok
test integer_bound_intervals_respect_discreteness ... ok
test math_rounding_is_decimal_exact ... ok
test multiline_bytes_dedent_handles_cr_and_interpolation_lines ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test materialized_hidden_fields_do_not_satisfy_quoted_optionals ... ok
test multi_expression_definitions_close_after_composition ... ok
test multiline_bytes_literals_dedent_and_interpolate ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test definitions_can_select_their_own_fields ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test named_definition_flattens_optional_field_materialization ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test nested_definitions_are_selectable ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test math_integral_results_keep_the_cue_kind ... ok
test nested_params_groups_see_lexical_siblings_through_paths ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_params_groups_see_outer_overrides ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test gateway_payload_rules_are_rejected_with_full_paths ... ok
test numeric_equality_and_float_export_match_cue ... ok
test nested_patterns_constrain_nested_fields ... ok
test multiline_strings_use_cue_dedent_and_cr_rules ... ok
test open_list_element_types_are_enforced ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test open_list_prefix_values_export_their_prefix ... ok
test nested_params_groups_and_generated_chains_avoid_false_cycles ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test optional_nested_definitions_are_not_selectable ... ok
test optional_intermediate_params_fields_are_rejected ... ok
test optional_definition_fields_are_not_materialized ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_values_get_patterns_before_closing ... ok
test open_list_prefixes_work_as_lists ... ok
test non_struct_definition_values_close_nested_structs ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test optional_value_conflicts_are_still_deferred ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test numeric_equality_is_scalar_only ... ok
test go_time_layouts_and_helpers_match_cue ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test overrides_keep_generated_optional_constraints ... ok
test overrides_are_visible_to_params_comprehensions ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test optional_fields_do_not_leak_through_unification_and_vars_shadow ... ok
test gateway_additional_validation_rules_are_path_qualified ... ok
test params_optional_and_shadowing_rules_hold ... ok
test params_cache_distinguishes_dotted_labels_from_paths ... ok
test params_items_can_depend_on_later_generated_siblings ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test open_list_prefixes_align_and_enforce_element_constraints ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test patterns_apply_to_declared_fields ... ok
test pattern_values_can_be_disjunctions ... ok
test params_dynamic_fields_hidden_definitions_and_quoted_selectors ... ok
test optional_open_metadata_on_struct_literals_matches_cue ... ok
test params_comprehensions_can_read_siblings_without_false_cycles ... ok
test parenthesized_params_keep_overrides ... ok
test params_dependency_chains_resolve_in_comprehensions ... ok
test quoted_hash_labels_are_regular_fields ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test params_siblings_numeric_literals_and_bytes_concat ... ok
test params_resolution_handles_nested_groups_lets_and_fields ... ok
test params_paths_resolve_against_the_params_root ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test quoted_hash_labels_are_selectable_from_struct_values ... ok
test quoted_parameter_names_use_identifier_values ... ok
test params_index_paths_resolve_through_nested_groups ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test live_struct_sources_and_constant_conditions_keep_cycles ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test renders_templates ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test rejects_missing_required_parameter ... ok
test quoted_and_index_selectors_do_not_reach_hidden_fields ... ok
test retry_int_shorthand_respects_the_cap ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test recursive_definitions_apply_nested_constraints ... ok
test quoted_params_lookup_does_not_return_cached_definitions ... ok
test step_level_unknown_fields_are_rejected ... ok
test self_selection_same_body_comprehension_is_a_documented_limit ... ok
test re2_quantifier_grammar_matches_cue ... ok
test nested_comprehension_bodies_shadow_in_static_liveness ... ok
test sentinel_prefixed_quoted_labels_select_their_escaped_storage ... ok
test regexp_quote_find_and_re2_classes_match_cue ... ok
test root_params_patterns_constrain_fields ... ok
test recursive_definitions_materialize_finite_values ... ok
test string_and_bytes_repetition_match_cue ... ok
test string_escapes_are_decoded_and_invalid_ones_rejected ... ok
test regexp_uses_ascii_classes_and_repeat_limits ... ok
test string_index_addresses_regular_fields_only ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test underscore_alias_is_rejected ... ok
test top_in_disjunctions_is_preserved ... ok
test recursive_definitions_fill_defaults_at_every_depth ... ok
test unicode_surrogates_and_string_escape_edges ... ok
test unknown_workflow_fields_are_rejected ... ok
test unicode_string_builtins_match_go_semantics ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test struct_string_indexes_respect_sentinel_namespaces ... ok
test time_fractional_layouts_and_duration_edges ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test time_garbage_and_duration_fraction_edges ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test unification_sources_are_live_when_either_side_is_live ... ok
test regex_class_complements_and_repeat_scanner_are_re2_compatible ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok
test time_helpers_handle_fractions_signs_and_nanos ... ok
test struct_literal_shadowing_keeps_dead_bodies_dead ... ok
test unresolved_disjunctions_are_rejected_in_boolean_contexts ... ok
test time_layouts_enforce_go_field_widths ... ok
test regexp_math_and_time_builtins_match_cue ... ok
test time_rfc3339_and_leap_seconds_are_strict ... ok
test required_fields_can_depend_on_materialized_optional_fields ... ok
test undemanded_let_cycles_are_rejected_statically ... ok
test deep_values_compare_and_materialize_beyond_128_levels ... ok

test result: ok. 238 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s

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

Rust vs CUE: optional/open literal metadata, comprehension-generated definitions, arbitrary-precision integers, primitive optional-dependent fields, self-selection, multiline strings and bytes, math/time, recursive materialization, @tag discovery, params, bounds/decimals, open lists, aliases, patterns, imports and workflow runtime.

````text
### ./scripts/conformance.sh

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running tests/conformance.rs (target/debug/deps/conformance-83e2b549b3c42212)

running 1 test
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.87s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=c4e8be48-5f77-484e-a2c2-b1ffeafdaaaa workflow=hello status=succeeded durationMs=14
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

run=125fc0d3-6836-4d74-bb42-cfb09e2250af workflow=retry status=succeeded durationMs=326
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=26d83803-abfb-4632-b23a-ac7770e7c127 workflow=cycle status=succeeded durationMs=93
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Visits/attempts.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status 26d83803-abfb-4632-b23a-ac7770e7c127

run=26d83803-abfb-4632-b23a-ac7770e7c127 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-04T00:02:41.826Z updated=2026-11-04T00:02:41.920Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs 26d83803-abfb-4632-b23a-ac7770e7c127

0 2026-11-04T00:02:41.826Z run.started step=- {"params":{},"stateHash":"a90a55f77a374d6b6bb941201f6c71ddf0591fe74deacf8d3b1ebc9e5ddf2423","workflow":"cycle"}
1 2026-11-04T00:02:41.847Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-04T00:02:41.847Z state.committed step=- {"resumeIndex":1,"stateHash":"45c178124b49006c62b57096d8f276349f80c0130429ccad51790f757aa7c072"}
3 2026-11-04T00:02:41.857Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
4 2026-11-04T00:02:41.857Z state.committed step=- {"resumeIndex":2,"stateHash":"b3f61f97ce8fd8f448d206ad0bb4929834dd80a499e79c0dfa43f0719e80bdcf"}
5 2026-11-04T00:02:41.873Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
6 2026-11-04T00:02:41.873Z state.committed step=- {"resumeIndex":3,"stateHash":"424254928ad2790338ce7e8ee5ee96aba1bdcb3d0a37a693fe1b2614d5a35c3e"}
7 2026-11-04T00:02:41.882Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
8 2026-11-04T00:02:41.882Z state.committed step=- {"resumeIndex":4,"stateHash":"db9bc3dc2700ca875d4d905abeed329e13580a64255cfec75c9a94c2027cd309"}
9 2026-11-04T00:02:41.896Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
10 2026-11-04T00:02:41.896Z state.committed step=- {"resumeIndex":5,"stateHash":"3fac015bd38e6e64f6077e3b36469c70459db04f5f868e967603b26d4984ea76"}
11 2026-11-04T00:02:41.905Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
12 2026-11-04T00:02:41.905Z state.committed step=- {"resumeIndex":6,"stateHash":"babdea534dac4ee2586b9935fea4615f48ee838a7a27aeb26c3463ce72ced152"}
13 2026-11-04T00:02:41.919Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
14 2026-11-04T00:02:41.919Z state.committed step=- {"resumeIndex":7,"stateHash":"575e25bb558c08b854e9cbc400c5eb6ae16859d59c06308fed2f73d9669dbc18"}
15 2026-11-04T00:02:41.919Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
16 2026-11-04T00:02:41.919Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}
17 2026-11-04T00:02:41.920Z run.succeeded step=- {"resumeIndex":11}
18 2026-11-04T00:02:41.920Z state.committed step=- {"resumeIndex":11,"stateHash":"db938daf1062e00dcfa2de68cf461befb9755366ca5b129a0694e51f1ab1bdbd"}

exit=0
````

## Trial log

One row per attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials 26d83803-abfb-4632-b23a-ac7770e7c127

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      18       
refine               0     0      succeeded 1.000      9        
review               0     0      succeeded 0.300      14       
refine               1     0      succeeded 1.000      7        
review               1     0      succeeded 0.600      13       
refine               2     0      succeeded 1.000      7        
review               2     0      succeeded 0.900      12       

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify 26d83803-abfb-4632-b23a-ac7770e7c127

OK run=26d83803-abfb-4632-b23a-ac7770e7c127 events=19 head=ccaae95121caea1441e72783402bd45bddcd9fe9d4ea0bc120c7b1c96d84ba6d

exit=0
````

## Metrics

Prometheus text.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run 26d83803-abfb-4632-b23a-ac7770e7c127 --format prometheus

# HELP ostinato_runs_total Runs started.
# TYPE ostinato_runs_total counter
ostinato_runs_total 1
ostinato_runs_succeeded_total 1
ostinato_runs_failed_total 0
ostinato_runs_running 0
ostinato_steps_total 3
ostinato_steps_failed_total 0
ostinato_steps_skipped_total 0
ostinato_gateways_evaluated_total 0
ostinato_attempts_total 7
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 11.43

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=37070cd3-3be2-4a23-a0fd-c53fbc135204 workflow=resume-evidence status=failed durationMs=13
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume 37070cd3-3be2-4a23-a0fd-c53fbc135204

run=37070cd3-3be2-4a23-a0fd-c53fbc135204 workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials 37070cd3-3be2-4a23-a0fd-c53fbc135204

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

run=9f5527ef-1a79-4c84-869c-9f385bd26604 workflow=agentic status=succeeded durationMs=33
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve 9f5527ef-1a79-4c84-869c-9f385bd26604 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=9f5527ef-1a79-4c84-869c-9f385bd26604 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 3811fc89-8938-42d1-b8e0-4b945fb99367 status=succeeded parent=9f5527ef-1a79-4c84-869c-9f385bd26604
improvement #4 run=3811fc89-8938-42d1-b8e0-4b945fb99367 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run a4705998-cd57-43c7-8bf3-8a5730670adb status=succeeded parent=3811fc89-8938-42d1-b8e0-4b945fb99367

exit=0
````

## Child runs

Parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
a4705998-cd57-43c7-8bf3-8a5730670adb   agentic          succeeded  3        2026-11-04T00:02:42.176Z
3811fc89-8938-42d1-b8e0-4b945fb99367   agentic          succeeded  3        2026-11-04T00:02:42.130Z
9f5527ef-1a79-4c84-869c-9f385bd26604   agentic          succeeded  3        2026-11-04T00:02:42.056Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=9f5527ef-1a79-4c84-869c-9f385bd26604: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````

## Gateways: run

XOR split/join with skipped branches.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite --cue cue run examples/gateway.cue

run=80dc6ba8-06a7-461a-b541-ded0f4c448e6 workflow=release-decision status=succeeded durationMs=20
STEP                     STATUS     VISITS ATTEMPTS
deployFast               succeeded  1      1       
deploySlow               skipped    0      0       
merge                    succeeded  1      0       
notify                   succeeded  1      1       
route                    succeeded  1      0       
test                     succeeded  1      1       

exit=0
````

## Gateways: status

Executed and skipped step rows.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite status 80dc6ba8-06a7-461a-b541-ded0f4c448e6

run=80dc6ba8-06a7-461a-b541-ded0f4c448e6 workflow=release-decision status=succeeded resumeIndex=6
source=./examples/gateway.cue
created=2026-11-04T00:02:42.224Z updated=2026-11-04T00:02:42.245Z
STEP                     STATUS     VISITS ATTEMPTS
deployFast               succeeded  1      1       
deploySlow               skipped    0      0       
merge                    succeeded  1      0       
notify                   succeeded  1      1       
route                    succeeded  1      0       
test                     succeeded  1      1       

exit=0
````

## Gateways: trials

Trial rows for executed and skipped steps.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite trials 80dc6ba8-06a7-461a-b541-ded0f4c448e6

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
test                 0     0      succeeded 0.950      5        
deployFast           0     0      succeeded 1.000      5        
notify               0     0      succeeded 1.000      4        

exit=0
````

## Gateways: audit stream

gateway.evaluated/joined and step.skipped events.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite logs 80dc6ba8-06a7-461a-b541-ded0f4c448e6

0 2026-11-04T00:02:42.224Z run.started step=- {"params":{"quality":"fast"},"stateHash":"3dc9e507f5fee848cb11d0fdd0ec4ea02760b2dfad48872f3992b6a7e5d6b806","workflow":"release-decision"}
1 2026-11-04T00:02:42.231Z step.succeeded step=test {"attempts":1,"error":null,"outputHash":"b49800e7e1ff4e636a523d8c8d15acdb0a27d35a871987f8a8a0bb774a1d0540","round":0,"score":0.95}
2 2026-11-04T00:02:42.231Z state.committed step=- {"resumeIndex":1,"stateHash":"e3d66a5b110b1bc8ae02d9872ed9b5c0bbcd5de46d668736f178394a48c63fef"}
3 2026-11-04T00:02:42.232Z gateway.evaluated step=route {"chosen":["deployFast"],"default":"deploySlow","evaluated":{"deployFast":{"condition":"steps.test.score >= 0.9 && params.quality == \"fast\"","result":true},"deploySlow":{"default":true,"result":true}},"gateway":"route","round":0,"split":"xor","usedDefault":false}
4 2026-11-04T00:02:42.232Z state.committed step=- {"resumeIndex":2,"stateHash":"980d5e2f36187540905f533d18f3952739bad56e8d0398186609eb20dace080a"}
5 2026-11-04T00:02:42.237Z step.succeeded step=deployFast {"attempts":1,"error":null,"outputHash":"ee0d753a9d044b6f11e644eec8d280f6a9f6231bf623315e23bc0766f8764e14","round":0,"score":1.0}
6 2026-11-04T00:02:42.237Z step.skipped step=deploySlow {"reason":"dead_path","round":0,"step":"deploySlow"}
7 2026-11-04T00:02:42.237Z state.committed step=- {"resumeIndex":4,"stateHash":"48f6313da9de78bdff4e890fbf070fdb12e03146071736fa76a6e19adefbd11d"}
8 2026-11-04T00:02:42.238Z gateway.joined step=merge {"active":["deployFast"],"dead":["deploySlow"],"decision":"taken","gateway":"merge","incoming":["deployFast","deploySlow"],"join":"or","round":0}
9 2026-11-04T00:02:42.238Z state.committed step=- {"resumeIndex":5,"stateHash":"e74b7b4fe527ac605c5fed71a1d27f0a1f1e19bd580c11341cf3ad1fa1829e02"}
10 2026-11-04T00:02:42.244Z step.succeeded step=notify {"attempts":1,"error":null,"outputHash":"25a50fab20e90ead1f539bad84b2e17ef771f4716ae4aa99ace3c914c70b51e4","round":0,"score":1.0}
11 2026-11-04T00:02:42.244Z state.committed step=- {"resumeIndex":6,"stateHash":"1ec1be21048468ff3c2972b859648870c0a84711f3acc490af387acbdbdde9a6"}
12 2026-11-04T00:02:42.245Z run.succeeded step=- {"resumeIndex":6}
13 2026-11-04T00:02:42.245Z state.committed step=- {"resumeIndex":6,"stateHash":"1ec1be21048468ff3c2972b859648870c0a84711f3acc490af387acbdbdde9a6"}

exit=0
````

## Gateways: audit verification

Hash chain and state digest with gateway routing.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite audit verify 80dc6ba8-06a7-461a-b541-ded0f4c448e6

OK run=80dc6ba8-06a7-461a-b541-ded0f4c448e6 events=14 head=1fc449f2f7dac32d6d245b2e33819c331b98e6edf4c72fc4d065fb2344088015

exit=0
````

## Gateways: metrics

steps_skipped / gateways_evaluated in Prometheus text.

````text
### ./target/debug/ostinato --db <workdir>/gateway.sqlite metrics --run 80dc6ba8-06a7-461a-b541-ded0f4c448e6 --format prometheus

# HELP ostinato_runs_total Runs started.
# TYPE ostinato_runs_total counter
ostinato_runs_total 1
ostinato_runs_succeeded_total 1
ostinato_runs_failed_total 0
ostinato_runs_running 0
ostinato_steps_total 6
ostinato_steps_failed_total 0
ostinato_steps_skipped_total 1
ostinato_gateways_evaluated_total 2
ostinato_attempts_total 3
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 4.67

exit=0
````
