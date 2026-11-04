# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.34.2
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
    Checking ostinato-domain v0.34.2 (./crates/ostinato-domain)
    Checking ostinato-lang v0.34.2 (./crates/ostinato-lang)
    Checking ostinato-application v0.34.2 (./crates/ostinato-application)
    Checking ostinato-adapters v0.34.2 (./crates/ostinato-adapters)
    Checking ostinato v0.34.2 (./crates/ostinato-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.73s
exit=0

### cargo test --all
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/main.rs (<target>/debug/deps/ostinato-ece30b7815fe49a8)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (<target>/debug/deps/conformance-3586ff64c06ae90e)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.89s

     Running tests/e2e.rs (<target>/debug/deps/e2e-905eeb629dd171e8)

running 102 tests
test bare_open_list_needs_and_params_compile ... ok
test duplicate_and_nested_tags_follow_cue ... ok
test agents_and_artifacts_error_on_unknown_runs ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test acyclic_runs_do_not_emit_cycle_events ... ok
test audit_verification_detects_tampering ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test definition_backed_tags_work_in_both_evaluators ... ok
test bad_template_is_a_durable_failure ... ok
test gateway_audit_detects_a_tampered_skipped_row ... ok
test gateway_audit_verifies_and_detects_tampered_gateway_output ... ok
test gateway_and_fan_out_and_fan_in_wait_for_both_branches ... ok
test gateway_graph_dot_and_json_expose_routing_metadata ... ok
test agent_trace_tampering_is_detected_by_audit_verify ... ok
test gateway_cycle_dead_carry_exits_and_skips_the_tail ... ok
test gateway_expression_join_runs_when_the_expression_is_true ... ok
test filtered_agents_verify_still_checks_the_whole_run ... ok
test gateway_failure_is_recorded_and_keeps_resume_index_at_the_gateway_group ... ok
test gateway_cycle_runs_rounds_and_audits ... ok
test gateway_cycle_round_tampering_fails_audit_verify ... ok
test cycles_stop_when_until_becomes_true ... ok
test gateway_dead_path_propagates_and_records_skips ... ok
test hidden_metadata_is_allowed_throughout_the_workflow_schema ... ok
test gateway_cycle_with_external_entry_survives_round_one ... ok
test gateway_n_of_partial_quorum_fails_the_run ... ok
test gateway_static_analysis_warns_and_strict_fails ... ok
test gateway_n_of_join_runs_when_quorum_is_met ... ok
test let_and_alias_backed_params_tags_work ... ok
test agent_definitions_produce_verifiable_traces_and_artifacts ... ok
test nested_definition_tags_inject_their_declaration ... ok
test gateway_or_runs_every_true_branch ... ok
test nested_tag_aliases_resolve_at_their_written_scope ... ok
test gateway_xor_default_branch_runs_when_no_condition_matches ... ok
test native_evaluator_supports_the_offline_subset ... ok
test non_struct_params_are_rejected ... ok
test gateway_routes_on_continue_on_error_status ... ok
test nested_tags_work_through_definitions_and_reject_bad_types ... ok
test nested_tags_cover_definitions_hidden_quoted_and_shorthand ... ok
test nested_tags_follow_label_identity_depth_and_expression_bases ... ok
test gateway_resume_after_split_does_not_re_evaluate_or_run_dead_branch ... ok
test package_underscore_files_are_file_local ... ok
test leaf_tag_injection_preserves_lexical_scopes ... ok
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test params_siblings_resolve_and_true_cycles_fail_cleanly ... ok
test multiline_and_interpolated_bytes_match_both_evaluators ... ok
test recursive_definitions_do_not_overflow_tag_collection ... ok
test jsonl_provider_output_drives_until_conditions ... ok
test legacy_run_without_gateways_still_resumes_and_verifies ... ok
test deleting_a_trace_row_is_detected_from_step_outputs ... ok
test gateway_xor_routes_one_branch_and_skips_the_other ... ok
test nested_tags_inject_nested_parameters ... ok
test agent_retries_keep_one_trace_per_attempt ... ok
test selector_tags_cover_leaves_duplicates_and_shadowed_definitions ... ok
test selector_tags_follow_definitions_duplicates_and_scope ... ok
test read_only_commands_do_not_require_cue ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test tag_attributes_drive_parameter_injection_types ... ok
test split_and_duplicate_definition_tags_work ... ok
test resume_refuses_a_recomputed_digest_that_is_not_anchored ... ok
test tag_identity_handles_shadowing_selectors_and_disjunctions ... ok
test cycle_rounds_each_get_a_trace ... ok
test recursive_definitions_materialize_in_both_evaluators ... ok
test tag_names_and_value_grammar_follow_cue ... ok
test tag_number_grammar_and_parenthesized_params ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test official_cue_runs_and_audit_verifies ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test tag_literal_grammar_and_strict_types ... ok
test official_cue_evaluator_resolves_agent_definitions ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test resume_extraction_namespaces_replayed_traces_by_id ... ok
test tool_command_handles_large_stdin_and_capture ... ok
test resume_replays_a_group_without_trace_key_collisions ... ok
test tool_command_nested_output_ignores_import_concrete_values ... ok
test tool_command_resolves_relative_dir_from_package ... ok
test tool_files_stay_excluded_from_normal_workflow_loading ... ok
test tool_command_unknown_tag_matches_cue ... ok
test unknown_fields_are_rejected ... ok
test truncation_blocks_resume_and_appends ... ok
test tool_command_let_selector_injection_is_an_ostinato_extension ... ok
test recursive_improvement_creates_child_runs ... ok
test validate_tolerates_unused_incomplete_module_fields ... ok
test tool_command_spawn_error_messages_match_cue ... ok
test tool_command_print_and_file_tasks_match_cue ... ok
test tool_command_referenced_and_unified_commands_match_cue ... ok
test tool_command_nested_task_outputs_match_cue ... ok
test tag_placement_violations_match_both_evaluators ... ok
test tool_command_rejects_conflicting_defaults ... ok
test exec_timeout_kills_the_process_group ... ok
test unused_imports_match_cue ... ok
test tool_command_selector_commands_match_cue ... ok
test tool_command_module_task_exports_match_cue ... ok
test worktree_isolation_keeps_the_checkout_clean_and_captures_changes ... ok
test tool_command_let_task_outputs_match_cue ... ok
test deep_recursive_values_do_not_exhaust_the_stack ... ok
test tool_command_let_and_definition_output_reads_match_cue ... ok
test tool_commands_match_cue ... ok
run=16c7f9e6-9c0e-44ce-899f-8911b35aa252 workflow=cancel status=cancelled durationMs=4026
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 102 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.11s

     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-e4da07fd8af187e0)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.22s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_adapters-6553c432d3c37668)

running 8 tests
test process::tests::explicit_command_and_delivery_override_the_provider ... ok
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok
test process::tests::arg_delivery_rejects_option_like_prompts ... ok
test process::tests::missing_command_provider_is_a_configuration_error ... ok
test process::tests::provider_argv_templates_are_stable ... ok
test process::tests::shell_commands_force_stdin_and_reject_arg_delivery ... ok
test process::timeout_tests::timed_out_agents_still_capture_output ... ok
test process::timeout_tests::escaped_descendants_do_not_erase_partial_output ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s

     Running tests/store.rs (<target>/debug/deps/store-87943904b3a6f1d7)

running 10 tests
test create_run_with_event_writes_the_start_anchor_atomically ... ok
test database_and_wal_files_are_private ... ok
test opening_a_v0_database_migrates_steps_round_and_backfills_integrity ... ok
test deleting_an_agent_event_breaks_trace_anchoring ... ok
test sqlite_store_roundtrips_step_rounds ... ok
test resume_run_is_atomic_and_refuses_terminal_runs ... ok
test agent_traces_roundtrip_and_verify ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok
test agent_trace_and_artifact_tampering_are_detected ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

     Running tests/workspace.rs (<target>/debug/deps/workspace-3fc5da3d4c03ac3a)

running 8 tests
test relative_path_validation_is_strict ... ok
test symlinks_and_hardlinks_are_not_captured ... ok
test declared_globs_are_sorted_and_deduplicated ... ok
test extraction_rejects_traversal_and_symlinked_parents ... ok
test worktree_base_injection_and_non_repos_fail_cleanly ... ok
test worktree_capture_sees_agent_commits_against_the_recorded_base ... ok
test worktree_lifecycle_captures_tracked_and_untracked_changes ... ok
test worktree_capture_is_relative_to_a_subdirectory_workflow ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_application-5df6fb372425f084)

running 22 tests
test activation::tests::and_split_selects_all_branches ... ok
test activation::tests::condition_evaluation_error_is_a_gateway_error ... ok
test activation::tests::legacy_multiple_needs_keep_and_semantics ... ok
test activation::tests::normal_step_with_dead_input_is_dead ... ok
test activation::tests::missing_predecessor_is_invalid_state ... ok
test activation::tests::and_join_requires_every_active_input ... ok
test activation::tests::or_join_needs_one_active_input ... ok
test activation::tests::or_no_true_no_default_fails ... ok
test activation::tests::or_none_uses_default ... ok
test activation::tests::round_one_carry_reads_previous_round_and_rejects_stale_rows ... ok
test activation::tests::or_selects_every_true_branch ... ok
test activation::tests::n_of_requires_quorum_and_fails_between_zero_and_n ... ok
test activation::tests::round_zero_carry_is_absent_and_root_entry_starts ... ok
test activation::tests::expression_join_evaluates_edges ... ok
test activation::tests::split_gateway_branch_choice_controls_edge_state ... ok
test activation::tests::xor_join_two_active_is_ambiguous ... ok
test activation::tests::xor_join_one_active_is_active ... ok
test activation::tests::xor_join_zero_active_is_dead ... ok
test activation::tests::xor_no_true_no_default_fails ... ok
test activation::tests::xor_no_true_uses_default ... ok
test activation::tests::xor_two_true_is_ambiguous ... ok
test activation::tests::xor_unique_true_selects_one ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_domain-a9b05e88f7c0a807)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/analysis.rs (<target>/debug/deps/analysis-8d6153ebd0a31ab4)

running 9 tests
test missing_default_is_a_note ... ok
test xor_join_over_non_exclusive_inputs_is_reported ... ok
test branch_that_does_not_reach_a_join_is_reported ... ok
test shared_branch_target_is_reported ... ok
test n_of_one_over_exclusive_branches_is_reachable ... ok
test nested_xor_exclusivity_is_understood ... ok
test xor_branches_feeding_an_and_join_are_reported ... ok
test sibling_branches_with_separate_joins_are_reported ... ok
test xor_branches_feeding_an_xor_join_are_valid ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (<target>/debug/deps/domain-849224a15b18433f)

running 22 tests
test branch_target_with_needs_is_rejected ... ok
test invalid_default_is_rejected ... ok
test cycles_require_an_explicit_policy ... ok
test agent_trace_hash_is_deterministic_and_content_sensitive ... ok
test implicit_join_with_gateways_is_rejected ... ok
test join_with_branches_is_rejected ... ok
test audit_chain_detects_tampering ... ok
test cycles_expand_into_bounded_rounds ... ok
test gateway_plan_orders_branches_and_joins ... ok
test independent_steps_share_a_parallel_group ... ok
test legacy_plan_shape_is_unchanged_without_gateways ... ok
test gateway_outside_a_legacy_cycle_is_allowed ... ok
test legacy_step_spec_deserializes_without_gateway_and_reserializes_identically ... ok
test gateway_cycle_plan_marks_round_scope_and_carry_edges ... ok
test externally_entered_gateway_cycle_keeps_the_external_edge_round_agnostic ... ok
test new_event_helper_is_ergonomic ... ok
test n_of_and_expression_joins_are_materialised ... ok
test legacy_state_digest_is_stable_across_the_gateway_change ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test split_and_join_together_is_rejected ... ok
test legacy_agent_specs_keep_their_state_digest_bytes ... ok
test unknown_branch_target_is_rejected ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_lang-c5ee41cf0fe52ca2)

running 3 tests
test gotime::tests::formats_durations ... ok
test gotime::tests::formats_go_layouts ... ok
test gotime::tests::parses_go_layouts ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/agents.rs (<target>/debug/deps/agents-f321ccfe7ae7bf93)

running 6 tests
test unreferenced_definitions_are_still_validated ... ok
test definition_prompt_and_string_shorthand_are_supported ... ok
test string_command_is_shell_but_argv_is_preserved ... ok
test agent_spec_serialization_stays_compatible_for_inline_steps ... ok
test definitions_are_merged_with_step_overrides ... ok
test invalid_agent_schemas_are_rejected ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (<target>/debug/deps/lang-6faf5f988937c19f)

running 246 tests
test aliases_bind_field_values ... ok
test acyclic_let_chains_are_accepted ... ok
test absolute_params_paths_do_not_shadow_into_nested_groups ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test attributes_parse_in_every_cue_position ... ok
test aliases_to_control_prefix_quoted_labels_resolve ... ok
test bare_open_lists_behave_as_empty_lists ... ok
test arithmetic_uses_a_disjunction_default ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test absolute_params_references_inside_generated_groups_resolve ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test bytes_literals_use_go_escape_rules ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test bounds_narrow_concrete_values_and_types ... ok
test bound_disjunctions_and_defaults_are_lattice_aware ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test chained_dynamic_groups_resolve_sibling_values ... ok
test arbitrary_precision_integer_literals_keep_exact_values ... ok
test closed_definitions_apply_to_nested_dynamic_and_let_params ... ok
test closed_definitions_stay_closed_through_params ... ok
test closedness_through_let_mediated_definition_bodies ... ok
test closed_definitions_are_checked_inside_referenced_bodies ... ok
test comprehension_clause_sequences_are_supported ... ok
test alias_shadowing_keeps_dead_bodies_dead ... ok
test bytes_interpolation_formats_scalars_and_rejects_incomplete ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test bounds_reject_out_of_range_values ... ok
test big_integer_bounds_and_builtins_match_cue ... ok
test comprehension_labels_use_displayed_names_for_control_prefixes ... ok
test compiles_workflow_and_resolves_parameters ... ok
test cross_conditional_params_groups_share_materialized_values ... ok
test composed_definitions_keep_referenced_closedness ... ok
test decimal_arithmetic_is_exact ... ok
test control_prefix_labels_match_patterns ... ok
test closed_definition_intersections_check_every_operand ... ok
test branch_condition_scope_is_enforced_at_compile_time ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test bytes_literal_field_labels_match_string_labels ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test comprehension_generated_definitions_are_selectable_and_hidden ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test comprehension_generated_declarations_merge_like_cue ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test definition_selectors_and_quoted_hash_fields_in_params_paths ... ok
test comprehension_generated_definitions_respect_caches_and_shadowing ... ok
test derived_definitions_keep_referenced_closedness ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test duplicate_disjunction_defaults_collapse ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test disjunction_defaults_narrow_and_export ... ok
test evaluates_conditions ... ok
test definition_disjunctions_keep_alternatives ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test dynamic_index_then_selector_resolves_params_paths ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test definition_backed_params_see_overrides_in_dependents ... ok
test division_by_zero_is_an_error_not_a_panic ... ok
test forward_let_references_are_lazy ... ok
test bounds_validate_operands_types_and_intervals ... ok
test dynamic_intermediate_and_repeated_segments_resolve ... ok
test compiles_every_split_and_join_mode ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test escaped_patterns_and_quoted_labels_decode_correctly ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test gateway_condition_scope_accepts_transitive_predecessors_and_ignores_string_literals ... ok
test escaped_slash_case_mapping_and_decimal_ranges ... ok
test generated_hidden_optionals_in_definitions_are_optional ... ok
test gateway_conditions_and_step_status_are_accepted ... ok
test gateway_compiles_identically_through_value_and_json ... ok
test gateway_cycles_and_implicit_joins_are_rejected_at_compile_time ... ok
test gateway_definition_backed_branches_compile_identically ... ok
test generated_hidden_optionals_do_not_hide_visible_sentinel_labels ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test gateway_n_of_and_expression_modes_compile ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test equality_requires_concrete_operands ... ok
test generated_hidden_params_are_not_reachable_through_quoted_selectors ... ok
test gateway_n_of_duplicate_needs_and_non_injective_labels_are_rejected ... ok
test generated_root_optionals_do_not_hide_definitions ... ok
test gateway_n_of_and_expression_validation_is_path_qualified ... ok
test hidden_fields_are_invisible_to_value_operations ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test generated_quoted_optionals_do_not_touch_materialized_definitions ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test imported_packages_drop_package_hidden_fields ... ok
test generated_quoted_hash_and_control_labels_are_selectable ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test generated_params_fields_respect_hidden_quoting_namespaces ... ok
test identifier_selectors_prefer_hidden_over_quoted_twins ... ok
test hidden_fields_are_distinct_from_quoted_underscore_fields ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test imported_hidden_fields_do_not_leak_into_the_importer ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test invalid_aliases_are_rejected ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test int_and_float_do_not_unify ... ok
test let_cycles_are_rejected ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test definitions_can_select_their_own_fields ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test hidden_fields_are_evaluated_selected_and_conflict_checked ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test list_comprehensions_are_supported ... ok
test missing_field_separators_are_rejected ... ok
test list_comprehensions_support_struct_bodies ... ok
test multiline_bytes_escaped_backslash_before_paren_is_literal ... ok
test integer_bound_intervals_respect_discreteness ... ok
test materialized_sentinel_fields_stay_required_and_conflict_checked ... ok
test materialized_hidden_fields_do_not_satisfy_quoted_optionals ... ok
test multiline_bytes_dedent_handles_cr_and_interpolation_lines ... ok
test math_rounding_is_decimal_exact ... ok
test multiline_bytes_literals_dedent_and_interpolate ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test multi_expression_definitions_close_after_composition ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test named_definition_flattens_optional_field_materialization ... ok
test nested_definitions_are_selectable ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test math_integral_results_keep_the_cue_kind ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_params_groups_see_lexical_siblings_through_paths ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test nested_params_groups_see_outer_overrides ... ok
test nested_multiline_literals_inside_interpolation_match_cue ... ok
test numeric_equality_and_float_export_match_cue ... ok
test nested_patterns_constrain_nested_fields ... ok
test gateway_payload_rules_are_rejected_with_full_paths ... ok
test open_list_element_types_are_enforced ... ok
test multiline_strings_use_cue_dedent_and_cr_rules ... ok
test open_list_prefix_values_export_their_prefix ... ok
test nested_params_groups_and_generated_chains_avoid_false_cycles ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test optional_definition_fields_are_not_materialized ... ok
test optional_nested_definitions_are_not_selectable ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test optional_intermediate_params_fields_are_rejected ... ok
test go_time_layouts_and_helpers_match_cue ... ok
test non_struct_definition_values_close_nested_structs ... ok
test gateway_additional_validation_rules_are_path_qualified ... ok
test numeric_equality_is_scalar_only ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test optional_value_conflicts_are_still_deferred ... ok
test optional_values_get_patterns_before_closing ... ok
test open_list_prefixes_work_as_lists ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test optional_fields_do_not_leak_through_unification_and_vars_shadow ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test overrides_are_visible_to_params_comprehensions ... ok
test overrides_keep_generated_optional_constraints ... ok
test open_list_prefixes_align_and_enforce_element_constraints ... ok
test params_cache_distinguishes_dotted_labels_from_paths ... ok
test params_optional_and_shadowing_rules_hold ... ok
test params_items_can_depend_on_later_generated_siblings ... ok
test optional_open_metadata_on_struct_literals_matches_cue ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test params_dynamic_fields_hidden_definitions_and_quoted_selectors ... ok
test pattern_values_can_be_disjunctions ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test params_comprehensions_can_read_siblings_without_false_cycles ... ok
test patterns_apply_to_declared_fields ... ok
test params_dependency_chains_resolve_in_comprehensions ... ok
test params_resolution_handles_nested_groups_lets_and_fields ... ok
test parenthesized_params_keep_overrides ... ok
test params_siblings_numeric_literals_and_bytes_concat ... ok
test quoted_hash_labels_are_regular_fields ... ok
test params_paths_resolve_against_the_params_root ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test live_struct_sources_and_constant_conditions_keep_cycles ... ok
test params_index_paths_resolve_through_nested_groups ... ok
test quoted_hash_labels_are_selectable_from_struct_values ... ok
test quoted_parameter_names_use_identifier_values ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test nested_comprehension_bodies_shadow_in_static_liveness ... ok
test renders_templates ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test quoted_and_index_selectors_do_not_reach_hidden_fields ... ok
test quoted_params_lookup_does_not_return_cached_definitions ... ok
test recursive_definitions_apply_nested_constraints ... ok
test rejects_missing_required_parameter ... ok
test retry_int_shorthand_respects_the_cap ... ok
test recursive_definitions_materialize_finite_values ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test regexp_quote_find_and_re2_classes_match_cue ... ok
test step_level_unknown_fields_are_rejected ... ok
test self_selection_same_body_comprehension_is_a_documented_limit ... ok
test root_params_patterns_constrain_fields ... ok
test re2_quantifier_grammar_matches_cue ... ok
test regexp_uses_ascii_classes_and_repeat_limits ... ok
test string_escapes_are_decoded_and_invalid_ones_rejected ... ok
test string_and_bytes_repetition_match_cue ... ok
test sentinel_prefixed_quoted_labels_select_their_escaped_storage ... ok
test string_index_addresses_regular_fields_only ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test underscore_alias_is_rejected ... ok
test struct_string_indexes_respect_sentinel_namespaces ... ok
test top_in_disjunctions_is_preserved ... ok
test recursive_definitions_fill_defaults_at_every_depth ... ok
test unicode_string_builtins_match_go_semantics ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test unicode_surrogates_and_string_escape_edges ... ok
test narrowed_defaults_recompute_dependent_fields ... ok
test time_fractional_layouts_and_duration_edges ... ok
test unknown_workflow_fields_are_rejected ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test time_garbage_and_duration_fraction_edges ... ok
test regex_class_complements_and_repeat_scanner_are_re2_compatible ... ok
test regexp_math_and_time_builtins_match_cue ... ok
test unification_sources_are_live_when_either_side_is_live ... ok
test time_helpers_handle_fractions_signs_and_nanos ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test struct_literal_shadowing_keeps_dead_bodies_dead ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok
test time_layouts_enforce_go_field_widths ... ok
test unresolved_disjunctions_are_rejected_in_boolean_contexts ... ok
test time_rfc3339_and_leap_seconds_are_strict ... ok
test required_fields_can_depend_on_materialized_optional_fields ... ok
test undemanded_let_cycles_are_rejected_statically ... ok
test unused_imports_are_rejected_like_cue ... ok
test tag_placement_rules_match_cue ... ok
test deep_values_compare_and_materialize_beyond_128_levels ... ok

test result: ok. 246 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s

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
### <target>/debug/ostinato --db <workdir>/db.sqlite validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=rust (CUE workflow semantics))

exit=0
````

## CUE oracle

Same workflow via CUE v0.17.1.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite --evaluator cue --cue cue validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=official CUE (cue version v0.17.1))

exit=0
````

## Conformance corpus

Rust vs CUE: tool commands, @tag placement, bytes-literal labels, nested multiline interpolation, default narrowing, optional/open literal metadata, comprehension-generated definitions, arbitrary-precision integers, primitive optional-dependent fields, self-selection, multiline strings and bytes, math/time, recursive materialization, params and workflow runtime.

````text
### ./scripts/conformance.sh

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running tests/conformance.rs (<target>/debug/deps/conformance-3586ff64c06ae90e)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.81s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-e4da07fd8af187e0)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.47s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=e924b976-ce65-4c89-98e1-ccc25dced679 workflow=hello status=succeeded durationMs=10
STEP                     STATUS     VISITS ATTEMPTS
audit                    succeeded  1      1       
greet                    succeeded  1      1       
prepare                  succeeded  1      1       

exit=0
````

## Retries

Fail twice, succeed on third attempt.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite run examples/retry.cue

run=ec7e5964-f519-4c4f-9aed-cfb0e93be502 workflow=retry status=succeeded durationMs=324
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=b198406e-de1a-4ba2-8342-986ccbad4ff8 workflow=cycle status=succeeded durationMs=89
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Run status

Visits/attempts.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite status b198406e-de1a-4ba2-8342-986ccbad4ff8

run=b198406e-de1a-4ba2-8342-986ccbad4ff8 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-04T18:31:34.918Z updated=2026-11-04T18:31:35.009Z
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite logs b198406e-de1a-4ba2-8342-986ccbad4ff8

0 2026-11-04T18:31:34.918Z run.started step=- {"params":{},"stateHash":"f61e5fcf882f75b5aef5bd035adcff414498073a56877e2b0f04214fda5f6e79","workflow":"cycle"}
1 2026-11-04T18:31:34.937Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-04T18:31:34.937Z state.committed step=- {"resumeIndex":1,"stateHash":"f8aff7b31875c41bc025e4a7d3c973dccafcf71d69958e9e049c1c18b7cd10d3"}
3 2026-11-04T18:31:34.953Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
4 2026-11-04T18:31:34.953Z state.committed step=- {"resumeIndex":2,"stateHash":"fb2f103e25d5910805212dc02e4c56386f7195e22d77802dc2aa40bd331eb731"}
5 2026-11-04T18:31:34.962Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
6 2026-11-04T18:31:34.962Z state.committed step=- {"resumeIndex":3,"stateHash":"26e51c218c7cbf114d4f9746b2f3102de0dacaa4bac271385de41d5614109565"}
7 2026-11-04T18:31:34.977Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
8 2026-11-04T18:31:34.977Z state.committed step=- {"resumeIndex":4,"stateHash":"e1f3cd7941a13deba7a034b3e7d8dd483a573bb23794c3dbcfb372b8b9c8febc"}
9 2026-11-04T18:31:34.986Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
10 2026-11-04T18:31:34.986Z state.committed step=- {"resumeIndex":5,"stateHash":"b917601c8f01c74702f0a417347c45e136da12208d17a789bb2645e84536d805"}
11 2026-11-04T18:31:35.000Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
12 2026-11-04T18:31:35.000Z state.committed step=- {"resumeIndex":6,"stateHash":"ba4d5c802f2d3aac4ae6ae26d8ef7ea6f7dfcd157683e1260714ab6c52d09524"}
13 2026-11-04T18:31:35.008Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
14 2026-11-04T18:31:35.008Z cycle.completed step=refine {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
15 2026-11-04T18:31:35.008Z state.committed step=- {"resumeIndex":11,"stateHash":"2280ce71f43300fa1865752685f077b0c6498a00aed958edea96e952d7b2567d"}
16 2026-11-04T18:31:35.009Z run.succeeded step=- {"resumeIndex":11}
17 2026-11-04T18:31:35.009Z state.committed step=- {"resumeIndex":11,"stateHash":"2280ce71f43300fa1865752685f077b0c6498a00aed958edea96e952d7b2567d"}

exit=0
````

## Trial log

One row per attempt.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite trials b198406e-de1a-4ba2-8342-986ccbad4ff8

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      16       
review               0     0      succeeded 0.300      14       
refine               0     0      succeeded 1.000      7        
review               1     0      succeeded 0.600      13       
refine               1     0      succeeded 1.000      7        
review               2     0      succeeded 0.900      12       
refine               2     0      succeeded 1.000      6        

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite audit verify b198406e-de1a-4ba2-8342-986ccbad4ff8

OK run=b198406e-de1a-4ba2-8342-986ccbad4ff8 events=18 head=e47706e2cc6e6e894fb2cc17741bb59461c6c8d6a03d74ec8c2a4b90be042268

exit=0
````

## Metrics

Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite metrics --run b198406e-de1a-4ba2-8342-986ccbad4ff8 --format prometheus

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
ostinato_gateway_failures_total 0
ostinato_gateway_quorum_short_total 0
ostinato_attempts_total 7
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 10.71

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=23fb0053-dd5f-4a44-9c0e-81a1d4d2b4dc workflow=resume-evidence status=failed durationMs=11
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite resume 23fb0053-dd5f-4a44-9c0e-81a1d4d2b4dc

run=23fb0053-dd5f-4a44-9c0e-81a1d4d2b4dc workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite trials 23fb0053-dd5f-4a44-9c0e-81a1d4d2b4dc

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      5        
b                    0     0      failed   0.000      4        command exited with 7: 
b                    0     0      succeeded 1.000      4        

exit=0
````

## Agentic workflow

agent + improve + report.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=b2de1598-97ad-4723-9896-fa2c9547f41b workflow=agentic status=succeeded durationMs=31
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite improve b2de1598-97ad-4723-9896-fa2c9547f41b --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=b2de1598-97ad-4723-9896-fa2c9547f41b diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 6e396c8c-b273-47c9-b65a-2f5e84245080 status=succeeded parent=b2de1598-97ad-4723-9896-fa2c9547f41b
improvement #4 run=6e396c8c-b273-47c9-b65a-2f5e84245080 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run a219208d-a910-4684-8651-56b44fe2eb1c status=succeeded parent=6e396c8c-b273-47c9-b65a-2f5e84245080

exit=0
````

## Child runs

Parent/child chain.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
a219208d-a910-4684-8651-56b44fe2eb1c   agentic          succeeded  3        2026-11-04T18:31:35.261Z
6e396c8c-b273-47c9-b65a-2f5e84245080   agentic          succeeded  3        2026-11-04T18:31:35.215Z
b2de1598-97ad-4723-9896-fa2c9547f41b   agentic          succeeded  3        2026-11-04T18:31:35.145Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=b2de1598-97ad-4723-9896-fa2c9547f41b: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````

## Gateways: run

XOR split/join with skipped branches.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite --cue cue run examples/gateway.cue

run=f05ee84e-3f57-4e80-acf8-4928aa7a773e workflow=release-decision status=succeeded durationMs=18
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite status f05ee84e-3f57-4e80-acf8-4928aa7a773e

run=f05ee84e-3f57-4e80-acf8-4928aa7a773e workflow=release-decision status=succeeded resumeIndex=6
source=./examples/gateway.cue
created=2026-11-04T18:31:35.307Z updated=2026-11-04T18:31:35.325Z
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite trials f05ee84e-3f57-4e80-acf8-4928aa7a773e

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
test                 0     0      succeeded 0.950      4        
deployFast           0     0      succeeded 1.000      4        
notify               0     0      succeeded 1.000      4        

exit=0
````

## Gateways: audit stream

gateway.evaluated/joined and step.skipped events.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite logs f05ee84e-3f57-4e80-acf8-4928aa7a773e

0 2026-11-04T18:31:35.307Z run.started step=- {"params":{"quality":"fast"},"stateHash":"7473c58f3c218cc20cb77292d754803b0173ce1a1009aafdaf15efb1db805e51","workflow":"release-decision"}
1 2026-11-04T18:31:35.313Z step.succeeded step=test {"attempts":1,"error":null,"outputHash":"b49800e7e1ff4e636a523d8c8d15acdb0a27d35a871987f8a8a0bb774a1d0540","round":0,"score":0.95}
2 2026-11-04T18:31:35.313Z state.committed step=- {"resumeIndex":1,"stateHash":"1e952f12722e54862c4b816dc9cfb5b94f4881a530a909405b4d9cfd7bab1997"}
3 2026-11-04T18:31:35.314Z gateway.evaluated step=route {"chosen":["deployFast"],"default":"deploySlow","evaluated":{"deployFast":{"condition":"steps.test.score >= 0.9 && params.quality == \"fast\"","result":true},"deploySlow":{"default":true,"result":true}},"gateway":"route","round":0,"split":"xor","usedDefault":false}
4 2026-11-04T18:31:35.314Z state.committed step=- {"resumeIndex":2,"stateHash":"8845929d0467bf3dd9ab90f4b66a680c6a3ed6d407ff7978a41d56dfbf2b5e53"}
5 2026-11-04T18:31:35.319Z step.succeeded step=deployFast {"attempts":1,"error":null,"outputHash":"ee0d753a9d044b6f11e644eec8d280f6a9f6231bf623315e23bc0766f8764e14","round":0,"score":1.0}
6 2026-11-04T18:31:35.319Z step.skipped step=deploySlow {"branch":"deploySlow","gateway":"route","reason":"dead_path","round":0,"step":"deploySlow"}
7 2026-11-04T18:31:35.319Z state.committed step=- {"resumeIndex":4,"stateHash":"05948082c235f23969ee9cdafbf417edeced3e5170a2792d5c0f550f676bd9ad"}
8 2026-11-04T18:31:35.319Z gateway.joined step=merge {"active":["deployFast"],"dead":["deploySlow"],"decision":"taken","gateway":"merge","incoming":["deployFast","deploySlow"],"join":"or","round":0}
9 2026-11-04T18:31:35.319Z state.committed step=- {"resumeIndex":5,"stateHash":"94c9c51a6c2eda12884e42d40c0b3b18c854d611dfb116dfb853c3b3ab613e36"}
10 2026-11-04T18:31:35.325Z step.succeeded step=notify {"attempts":1,"error":null,"outputHash":"25a50fab20e90ead1f539bad84b2e17ef771f4716ae4aa99ace3c914c70b51e4","round":0,"score":1.0}
11 2026-11-04T18:31:35.325Z state.committed step=- {"resumeIndex":6,"stateHash":"c8ccda442e234e07fb7b70addfc157db33f55f3b56585bb1248841bf670f2d4a"}
12 2026-11-04T18:31:35.325Z run.succeeded step=- {"resumeIndex":6}
13 2026-11-04T18:31:35.325Z state.committed step=- {"resumeIndex":6,"stateHash":"c8ccda442e234e07fb7b70addfc157db33f55f3b56585bb1248841bf670f2d4a"}

exit=0
````

## Gateways: audit verification

Hash chain and state digest with gateway routing.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite audit verify f05ee84e-3f57-4e80-acf8-4928aa7a773e

OK run=f05ee84e-3f57-4e80-acf8-4928aa7a773e events=14 head=6e2d87ab1f54fefc911ad823e481b330d8458f9c03d623fca340a7f0f2fe2884

exit=0
````

## Gateways: metrics

steps_skipped / gateways_evaluated in Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite metrics --run f05ee84e-3f57-4e80-acf8-4928aa7a773e --format prometheus

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
ostinato_gateway_failures_total 0
ostinato_gateway_quorum_short_total 0
ostinato_attempts_total 3
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 4.00

exit=0
````
