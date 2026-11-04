# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.37.3
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
     Running unittests src/main.rs (<target>/debug/deps/ostinato-e1b5b8990aa063d8)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (<target>/debug/deps/conformance-c30cfdbe2a2d9068)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.80s

     Running tests/e2e.rs (<target>/debug/deps/e2e-0c0fd9983440e2e3)

running 106 tests
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
test gateway_and_fan_out_and_fan_in_wait_for_both_branches ... ok
test gateway_audit_verifies_and_detects_tampered_gateway_output ... ok
test gateway_cycle_dead_carry_exits_and_skips_the_tail ... ok
test gateway_expression_join_runs_when_the_expression_is_true ... ok
test gateway_graph_dot_and_json_expose_routing_metadata ... ok
test gateway_cycle_round_tampering_fails_audit_verify ... ok
test deleting_a_trace_row_is_detected_from_step_outputs ... ok
test gateway_failure_is_recorded_and_keeps_resume_index_at_the_gateway_group ... ok
test gateway_cycle_runs_rounds_and_audits ... ok
test cycles_stop_when_until_becomes_true ... ok
test gateway_dead_path_propagates_and_records_skips ... ok
test filtered_agents_verify_still_checks_the_whole_run ... ok
test gateway_static_analysis_warns_and_strict_fails ... ok
test gateway_cycle_with_external_entry_survives_round_one ... ok
test hidden_metadata_is_allowed_throughout_the_workflow_schema ... ok
test gateway_n_of_partial_quorum_fails_the_run ... ok
test let_and_alias_backed_params_tags_work ... ok
test gateway_n_of_join_runs_when_quorum_is_met ... ok
test gateway_or_runs_every_true_branch ... ok
test nested_definition_tags_inject_their_declaration ... ok
test agent_trace_tampering_is_detected_by_audit_verify ... ok
test gateway_xor_default_branch_runs_when_no_condition_matches ... ok
test nested_tag_aliases_resolve_at_their_written_scope ... ok
test agent_definitions_produce_verifiable_traces_and_artifacts ... ok
test gateway_routes_on_continue_on_error_status ... ok
test native_evaluator_supports_the_offline_subset ... ok
test non_struct_params_are_rejected ... ok
test nested_tags_cover_definitions_hidden_quoted_and_shorthand ... ok
test nested_tags_work_through_definitions_and_reject_bad_types ... ok
test nested_tags_follow_label_identity_depth_and_expression_bases ... ok
test package_underscore_files_are_file_local ... ok
test leaf_tag_injection_preserves_lexical_scopes ... ok
test gateway_resume_after_split_does_not_re_evaluate_or_run_dead_branch ... ok
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test params_siblings_resolve_and_true_cycles_fail_cleanly ... ok
test recursive_definitions_do_not_overflow_tag_collection ... ok
test multiline_and_interpolated_bytes_match_both_evaluators ... ok
test legacy_run_without_gateways_still_resumes_and_verifies ... ok
test nested_tags_inject_nested_parameters ... ok
test jsonl_provider_output_drives_until_conditions ... ok
test gateway_xor_routes_one_branch_and_skips_the_other ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test selector_tags_cover_leaves_duplicates_and_shadowed_definitions ... ok
test selector_tags_follow_definitions_duplicates_and_scope ... ok
test read_only_commands_do_not_require_cue ... ok
test resume_refuses_a_recomputed_digest_that_is_not_anchored ... ok
test split_and_duplicate_definition_tags_work ... ok
test recursive_definitions_materialize_in_both_evaluators ... ok
test tag_attributes_drive_parameter_injection_types ... ok
test tag_identity_handles_shadowing_selectors_and_disjunctions ... ok
test agent_retries_keep_one_trace_per_attempt ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test official_cue_runs_and_audit_verifies ... ok
test tag_number_grammar_and_parenthesized_params ... ok
test tag_names_and_value_grammar_follow_cue ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test cycle_rounds_each_get_a_trace ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test tag_literal_grammar_and_strict_types ... ok
test resume_extraction_namespaces_replayed_traces_by_id ... ok
test official_cue_evaluator_resolves_agent_definitions ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test resume_replays_a_group_without_trace_key_collisions ... ok
test tool_command_handles_large_stdin_and_capture ... ok
test tool_command_nested_output_ignores_import_concrete_values ... ok
test tool_command_resolves_relative_dir_from_package ... ok
test tool_command_relative_paths_use_invocation_cwd ... ok
test recursive_improvement_creates_child_runs ... ok
test tool_command_let_selector_injection_is_an_ostinato_extension ... ok
test tool_command_unknown_tag_matches_cue ... ok
test tool_command_print_and_file_tasks_match_cue ... ok
test tool_files_stay_excluded_from_normal_workflow_loading ... ok
test unknown_fields_are_rejected ... ok
test truncation_blocks_resume_and_appends ... ok
test tool_command_referenced_and_unified_commands_match_cue ... ok
test tool_command_spawn_error_messages_match_cue ... ok
test tool_command_nested_task_outputs_match_cue ... ok
test validate_tolerates_unused_incomplete_module_fields ... ok
test tag_placement_violations_match_both_evaluators ... ok
test exec_timeout_kills_the_process_group ... ok
test tool_command_package_after_multi_task_matches_cue ... ok
test tool_command_rejects_conflicting_defaults ... ok
test unused_imports_match_cue ... ok
test tool_command_selector_commands_match_cue ... ok
test tool_command_module_task_exports_match_cue ... ok
test worktree_isolation_keeps_the_checkout_clean_and_captures_changes ... ok
test tool_command_let_task_outputs_match_cue ... ok
test tool_command_nontask_after_operands_match_cue ... ok
test tool_command_package_after_schedule_matches_cue ... ok
test deep_recursive_values_do_not_exhaust_the_stack ... ok
test tool_command_let_and_definition_output_reads_match_cue ... ok
test tool_commands_match_cue ... ok
run=8d16057d-d21e-44fe-bd7d-eecd11ad47fe workflow=cancel status=cancelled durationMs=4027
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 106 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.09s

     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-863d541595db53d2)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.89s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_adapters-700e78373696ce9d)

running 8 tests
test process::tests::missing_command_provider_is_a_configuration_error ... ok
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok
test process::tests::arg_delivery_rejects_option_like_prompts ... ok
test process::tests::explicit_command_and_delivery_override_the_provider ... ok
test process::tests::provider_argv_templates_are_stable ... ok
test process::tests::shell_commands_force_stdin_and_reject_arg_delivery ... ok
test process::timeout_tests::timed_out_agents_still_capture_output ... ok
test process::timeout_tests::escaped_descendants_do_not_erase_partial_output ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s

     Running tests/store.rs (<target>/debug/deps/store-a13ab4c3b5cc9e9a)

running 10 tests
test database_and_wal_files_are_private ... ok
test sqlite_store_roundtrips_step_rounds ... ok
test create_run_with_event_writes_the_start_anchor_atomically ... ok
test deleting_an_agent_event_breaks_trace_anchoring ... ok
test agent_traces_roundtrip_and_verify ... ok
test resume_run_is_atomic_and_refuses_terminal_runs ... ok
test opening_a_v0_database_migrates_steps_round_and_backfills_integrity ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok
test agent_trace_and_artifact_tampering_are_detected ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

     Running tests/workspace.rs (<target>/debug/deps/workspace-597037987914f80b)

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

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_application-e3bca12e0835c6be)

running 22 tests
test activation::tests::and_split_selects_all_branches ... ok
test activation::tests::legacy_multiple_needs_keep_and_semantics ... ok
test activation::tests::and_join_requires_every_active_input ... ok
test activation::tests::condition_evaluation_error_is_a_gateway_error ... ok
test activation::tests::expression_join_evaluates_edges ... ok
test activation::tests::normal_step_with_dead_input_is_dead ... ok
test activation::tests::or_join_needs_one_active_input ... ok
test activation::tests::missing_predecessor_is_invalid_state ... ok
test activation::tests::n_of_requires_quorum_and_fails_between_zero_and_n ... ok
test activation::tests::or_none_uses_default ... ok
test activation::tests::or_no_true_no_default_fails ... ok
test activation::tests::or_selects_every_true_branch ... ok
test activation::tests::round_one_carry_reads_previous_round_and_rejects_stale_rows ... ok
test activation::tests::split_gateway_branch_choice_controls_edge_state ... ok
test activation::tests::xor_join_one_active_is_active ... ok
test activation::tests::round_zero_carry_is_absent_and_root_entry_starts ... ok
test activation::tests::xor_join_two_active_is_ambiguous ... ok
test activation::tests::xor_join_zero_active_is_dead ... ok
test activation::tests::xor_no_true_no_default_fails ... ok
test activation::tests::xor_no_true_uses_default ... ok
test activation::tests::xor_two_true_is_ambiguous ... ok
test activation::tests::xor_unique_true_selects_one ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_domain-a4f579e2b5618f1b)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/analysis.rs (<target>/debug/deps/analysis-1bd07768ca595477)

running 9 tests
test missing_default_is_a_note ... ok
test xor_branches_feeding_an_and_join_are_reported ... ok
test xor_join_over_non_exclusive_inputs_is_reported ... ok
test xor_branches_feeding_an_xor_join_are_valid ... ok
test branch_that_does_not_reach_a_join_is_reported ... ok
test n_of_one_over_exclusive_branches_is_reachable ... ok
test sibling_branches_with_separate_joins_are_reported ... ok
test shared_branch_target_is_reported ... ok
test nested_xor_exclusivity_is_understood ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (<target>/debug/deps/domain-d2c05bfc8c9a05b7)

running 22 tests
test branch_target_with_needs_is_rejected ... ok
test invalid_default_is_rejected ... ok
test agent_trace_hash_is_deterministic_and_content_sensitive ... ok
test implicit_join_with_gateways_is_rejected ... ok
test cycles_require_an_explicit_policy ... ok
test join_with_branches_is_rejected ... ok
test audit_chain_detects_tampering ... ok
test independent_steps_share_a_parallel_group ... ok
test cycles_expand_into_bounded_rounds ... ok
test legacy_step_spec_deserializes_without_gateway_and_reserializes_identically ... ok
test gateway_plan_orders_branches_and_joins ... ok
test new_event_helper_is_ergonomic ... ok
test legacy_agent_specs_keep_their_state_digest_bytes ... ok
test gateway_cycle_plan_marks_round_scope_and_carry_edges ... ok
test legacy_plan_shape_is_unchanged_without_gateways ... ok
test externally_entered_gateway_cycle_keeps_the_external_edge_round_agnostic ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test gateway_outside_a_legacy_cycle_is_allowed ... ok
test n_of_and_expression_joins_are_materialised ... ok
test split_and_join_together_is_rejected ... ok
test unknown_branch_target_is_rejected ... ok
test legacy_state_digest_is_stable_across_the_gateway_change ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_lang-1dcd6006514a87db)

running 3 tests
test gotime::tests::formats_durations ... ok
test gotime::tests::formats_go_layouts ... ok
test gotime::tests::parses_go_layouts ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/agents.rs (<target>/debug/deps/agents-966f07715c85e45e)

running 6 tests
test unreferenced_definitions_are_still_validated ... ok
test definition_prompt_and_string_shorthand_are_supported ... ok
test agent_spec_serialization_stays_compatible_for_inline_steps ... ok
test string_command_is_shell_but_argv_is_preserved ... ok
test definitions_are_merged_with_step_overrides ... ok
test invalid_agent_schemas_are_rejected ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (<target>/debug/deps/lang-bc6c64ec956eb131)

running 246 tests
test acyclic_let_chains_are_accepted ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test aliases_bind_field_values ... ok
test absolute_params_paths_do_not_shadow_into_nested_groups ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test aliases_to_control_prefix_quoted_labels_resolve ... ok
test arithmetic_uses_a_disjunction_default ... ok
test attributes_parse_in_every_cue_position ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test bare_open_lists_behave_as_empty_lists ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test absolute_params_references_inside_generated_groups_resolve ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test bytes_literals_use_go_escape_rules ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test arbitrary_precision_integer_literals_keep_exact_values ... ok
test bound_disjunctions_and_defaults_are_lattice_aware ... ok
test bounds_narrow_concrete_values_and_types ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test closed_definitions_stay_closed_through_params ... ok
test chained_dynamic_groups_resolve_sibling_values ... ok
test closedness_through_let_mediated_definition_bodies ... ok
test closed_definitions_apply_to_nested_dynamic_and_let_params ... ok
test closed_definitions_are_checked_inside_referenced_bodies ... ok
test bytes_interpolation_formats_scalars_and_rejects_incomplete ... ok
test alias_shadowing_keeps_dead_bodies_dead ... ok
test comprehension_clause_sequences_are_supported ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test compiles_workflow_and_resolves_parameters ... ok
test composed_definitions_keep_referenced_closedness ... ok
test comprehension_labels_use_displayed_names_for_control_prefixes ... ok
test bounds_reject_out_of_range_values ... ok
test big_integer_bounds_and_builtins_match_cue ... ok
test cross_conditional_params_groups_share_materialized_values ... ok
test decimal_arithmetic_is_exact ... ok
test control_prefix_labels_match_patterns ... ok
test bytes_literal_field_labels_match_string_labels ... ok
test closed_definition_intersections_check_every_operand ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test comprehension_generated_declarations_merge_like_cue ... ok
test branch_condition_scope_is_enforced_at_compile_time ... ok
test comprehension_generated_definitions_are_selectable_and_hidden ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test derived_definitions_keep_referenced_closedness ... ok
test definition_selectors_and_quoted_hash_fields_in_params_paths ... ok
test disjunction_defaults_narrow_and_export ... ok
test comprehension_generated_definitions_respect_caches_and_shadowing ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test definition_disjunctions_keep_alternatives ... ok
test duplicate_disjunction_defaults_collapse ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test evaluates_conditions ... ok
test definition_backed_params_see_overrides_in_dependents ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test division_by_zero_is_an_error_not_a_panic ... ok
test dynamic_index_then_selector_resolves_params_paths ... ok
test forward_let_references_are_lazy ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test escaped_patterns_and_quoted_labels_decode_correctly ... ok
test dynamic_intermediate_and_repeated_segments_resolve ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test compiles_every_split_and_join_mode ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test bounds_validate_operands_types_and_intervals ... ok
test gateway_condition_scope_accepts_transitive_predecessors_and_ignores_string_literals ... ok
test escaped_slash_case_mapping_and_decimal_ranges ... ok
test gateway_conditions_and_step_status_are_accepted ... ok
test gateway_definition_backed_branches_compile_identically ... ok
test generated_hidden_optionals_in_definitions_are_optional ... ok
test gateway_compiles_identically_through_value_and_json ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test generated_hidden_optionals_do_not_hide_visible_sentinel_labels ... ok
test gateway_cycles_and_implicit_joins_are_rejected_at_compile_time ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test gateway_n_of_and_expression_modes_compile ... ok
test equality_requires_concrete_operands ... ok
test generated_hidden_params_are_not_reachable_through_quoted_selectors ... ok
test gateway_n_of_duplicate_needs_and_non_injective_labels_are_rejected ... ok
test generated_root_optionals_do_not_hide_definitions ... ok
test hidden_fields_are_invisible_to_value_operations ... ok
test gateway_n_of_and_expression_validation_is_path_qualified ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test generated_quoted_hash_and_control_labels_are_selectable ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test generated_quoted_optionals_do_not_touch_materialized_definitions ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test imported_packages_drop_package_hidden_fields ... ok
test generated_params_fields_respect_hidden_quoting_namespaces ... ok
test identifier_selectors_prefer_hidden_over_quoted_twins ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test imported_hidden_fields_do_not_leak_into_the_importer ... ok
test hidden_fields_are_distinct_from_quoted_underscore_fields ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test invalid_aliases_are_rejected ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test int_and_float_do_not_unify ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test let_cycles_are_rejected ... ok
test hidden_fields_are_evaluated_selected_and_conflict_checked ... ok
test definitions_can_select_their_own_fields ... ok
test list_comprehensions_are_supported ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test missing_field_separators_are_rejected ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test list_comprehensions_support_struct_bodies ... ok
test materialized_sentinel_fields_stay_required_and_conflict_checked ... ok
test multiline_bytes_escaped_backslash_before_paren_is_literal ... ok
test math_rounding_is_decimal_exact ... ok
test multiline_bytes_dedent_handles_cr_and_interpolation_lines ... ok
test integer_bound_intervals_respect_discreteness ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test multiline_bytes_literals_dedent_and_interpolate ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test materialized_hidden_fields_do_not_satisfy_quoted_optionals ... ok
test multi_expression_definitions_close_after_composition ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test math_integral_results_keep_the_cue_kind ... ok
test named_definition_flattens_optional_field_materialization ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test nested_definitions_are_selectable ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test nested_params_groups_see_lexical_siblings_through_paths ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test nested_multiline_literals_inside_interpolation_match_cue ... ok
test nested_params_groups_see_outer_overrides ... ok
test multiline_strings_use_cue_dedent_and_cr_rules ... ok
test numeric_equality_and_float_export_match_cue ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test open_list_element_types_are_enforced ... ok
test open_list_prefix_values_export_their_prefix ... ok
test nested_patterns_constrain_nested_fields ... ok
test nested_params_groups_and_generated_chains_avoid_false_cycles ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test optional_definition_fields_are_not_materialized ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test gateway_payload_rules_are_rejected_with_full_paths ... ok
test optional_intermediate_params_fields_are_rejected ... ok
test go_time_layouts_and_helpers_match_cue ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_nested_definitions_are_not_selectable ... ok
test numeric_equality_is_scalar_only ... ok
test open_list_prefixes_work_as_lists ... ok
test non_struct_definition_values_close_nested_structs ... ok
test optional_values_get_patterns_before_closing ... ok
test optional_value_conflicts_are_still_deferred ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test open_list_prefixes_align_and_enforce_element_constraints ... ok
test optional_fields_do_not_leak_through_unification_and_vars_shadow ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test gateway_additional_validation_rules_are_path_qualified ... ok
test overrides_are_visible_to_params_comprehensions ... ok
test params_optional_and_shadowing_rules_hold ... ok
test overrides_keep_generated_optional_constraints ... ok
test params_cache_distinguishes_dotted_labels_from_paths ... ok
test params_items_can_depend_on_later_generated_siblings ... ok
test params_dynamic_fields_hidden_definitions_and_quoted_selectors ... ok
test optional_open_metadata_on_struct_literals_matches_cue ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test params_comprehensions_can_read_siblings_without_false_cycles ... ok
test patterns_apply_to_declared_fields ... ok
test params_dependency_chains_resolve_in_comprehensions ... ok
test pattern_values_can_be_disjunctions ... ok
test params_resolution_handles_nested_groups_lets_and_fields ... ok
test params_paths_resolve_against_the_params_root ... ok
test live_struct_sources_and_constant_conditions_keep_cycles ... ok
test params_siblings_numeric_literals_and_bytes_concat ... ok
test quoted_hash_labels_are_regular_fields ... ok
test parenthesized_params_keep_overrides ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test nested_comprehension_bodies_shadow_in_static_liveness ... ok
test quoted_hash_labels_are_selectable_from_struct_values ... ok
test quoted_parameter_names_use_identifier_values ... ok
test params_index_paths_resolve_through_nested_groups ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test renders_templates ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test quoted_and_index_selectors_do_not_reach_hidden_fields ... ok
test quoted_params_lookup_does_not_return_cached_definitions ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test recursive_definitions_materialize_finite_values ... ok
test retry_int_shorthand_respects_the_cap ... ok
test rejects_missing_required_parameter ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test recursive_definitions_apply_nested_constraints ... ok
test self_selection_same_body_comprehension_is_a_documented_limit ... ok
test string_escapes_are_decoded_and_invalid_ones_rejected ... ok
test step_level_unknown_fields_are_rejected ... ok
test regexp_quote_find_and_re2_classes_match_cue ... ok
test re2_quantifier_grammar_matches_cue ... ok
test string_and_bytes_repetition_match_cue ... ok
test string_index_addresses_regular_fields_only ... ok
test sentinel_prefixed_quoted_labels_select_their_escaped_storage ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test regexp_uses_ascii_classes_and_repeat_limits ... ok
test root_params_patterns_constrain_fields ... ok
test underscore_alias_is_rejected ... ok
test top_in_disjunctions_is_preserved ... ok
test struct_string_indexes_respect_sentinel_namespaces ... ok
test recursive_definitions_fill_defaults_at_every_depth ... ok
test regexp_math_and_time_builtins_match_cue ... ok
test unicode_surrogates_and_string_escape_edges ... ok
test unicode_string_builtins_match_go_semantics ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test unknown_workflow_fields_are_rejected ... ok
test time_garbage_and_duration_fraction_edges ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test narrowed_defaults_recompute_dependent_fields ... ok
test struct_literal_shadowing_keeps_dead_bodies_dead ... ok
test time_fractional_layouts_and_duration_edges ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test unification_sources_are_live_when_either_side_is_live ... ok
test regex_class_complements_and_repeat_scanner_are_re2_compatible ... ok
test time_helpers_handle_fractions_signs_and_nanos ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok
test unresolved_disjunctions_are_rejected_in_boolean_contexts ... ok
test time_layouts_enforce_go_field_widths ... ok
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
     Running tests/conformance.rs (<target>/debug/deps/conformance-c30cfdbe2a2d9068)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.83s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-863d541595db53d2)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.17s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=b98ee996-85d4-41b3-b778-f65d40f5ac22 workflow=hello status=succeeded durationMs=10
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

run=f156bdbd-737d-43b9-b4d6-461954bb9059 workflow=retry status=succeeded durationMs=327
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=e9c5dd5d-0e77-4919-b13c-6fb939d2972a workflow=cycle status=succeeded durationMs=87
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Run status

Visits/attempts.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite status e9c5dd5d-0e77-4919-b13c-6fb939d2972a

run=e9c5dd5d-0e77-4919-b13c-6fb939d2972a workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-04T20:58:41.415Z updated=2026-11-04T20:58:41.502Z
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite logs e9c5dd5d-0e77-4919-b13c-6fb939d2972a

0 2026-11-04T20:58:41.415Z run.started step=- {"params":{},"stateHash":"8022ab548c8bfb59843b87e8e67ab0db037277e148de73fc9d99f51528c74252","workflow":"cycle"}
1 2026-11-04T20:58:41.434Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-04T20:58:41.434Z state.committed step=- {"resumeIndex":1,"stateHash":"872b9b063f8769b260dba16815dae8a7a68ac0ffec3bab38dbb8d7714a3c4f64"}
3 2026-11-04T20:58:41.449Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
4 2026-11-04T20:58:41.449Z state.committed step=- {"resumeIndex":2,"stateHash":"e539366f9e69489dfde0762239e81a0b2e55db2a01420339d3da462306134559"}
5 2026-11-04T20:58:41.458Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
6 2026-11-04T20:58:41.458Z state.committed step=- {"resumeIndex":3,"stateHash":"f135d227eec067ef436a494d9d3dfd4bce2b9b178a0d904033160e63657b902f"}
7 2026-11-04T20:58:41.472Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
8 2026-11-04T20:58:41.472Z state.committed step=- {"resumeIndex":4,"stateHash":"762f9a512a1673bcdc4e2c28b68c9abb18b4cc2fe90dd663fab7a098b8bece3c"}
9 2026-11-04T20:58:41.480Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
10 2026-11-04T20:58:41.480Z state.committed step=- {"resumeIndex":5,"stateHash":"4f0b9f9617010b3bb2a8b658efa50998c90359dc053c67ece75b116f5cebc142"}
11 2026-11-04T20:58:41.494Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
12 2026-11-04T20:58:41.494Z state.committed step=- {"resumeIndex":6,"stateHash":"165c6d97ed31fc9adf0c96a5f84930cfe70d07a5290a059cbe718700834895bd"}
13 2026-11-04T20:58:41.502Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
14 2026-11-04T20:58:41.502Z cycle.completed step=refine {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
15 2026-11-04T20:58:41.502Z state.committed step=- {"resumeIndex":11,"stateHash":"12fa6d63fba941947ce24fe9bffb3acfe15248a1f7732e0a054666f0e8e423fd"}
16 2026-11-04T20:58:41.502Z run.succeeded step=- {"resumeIndex":11}
17 2026-11-04T20:58:41.502Z state.committed step=- {"resumeIndex":11,"stateHash":"12fa6d63fba941947ce24fe9bffb3acfe15248a1f7732e0a054666f0e8e423fd"}

exit=0
````

## Trial log

One row per attempt.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite trials e9c5dd5d-0e77-4919-b13c-6fb939d2972a

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      17       
review               0     0      succeeded 0.300      13       
refine               0     0      succeeded 1.000      7        
review               1     0      succeeded 0.600      13       
refine               1     0      succeeded 1.000      6        
review               2     0      succeeded 0.900      12       
refine               2     0      succeeded 1.000      6        

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite audit verify e9c5dd5d-0e77-4919-b13c-6fb939d2972a

OK run=e9c5dd5d-0e77-4919-b13c-6fb939d2972a events=18 head=8d24d6b2f54ab556d74908dd8c8ac94dbd856e9b9a91336353137c6090eabef7

exit=0
````

## Metrics

Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite metrics --run e9c5dd5d-0e77-4919-b13c-6fb939d2972a --format prometheus

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
ostinato_attempt_duration_ms_avg 10.57

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=c16e46fb-ba32-4217-b060-3e36bf4d376d workflow=resume-evidence status=failed durationMs=11
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite resume c16e46fb-ba32-4217-b060-3e36bf4d376d

run=c16e46fb-ba32-4217-b060-3e36bf4d376d workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite trials c16e46fb-ba32-4217-b060-3e36bf4d376d

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

run=c063e462-572a-48b0-8502-9235ece9ee90 workflow=agentic status=succeeded durationMs=32
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite improve c063e462-572a-48b0-8502-9235ece9ee90 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=c063e462-572a-48b0-8502-9235ece9ee90 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run b47a22e5-5131-4127-977b-6295e7aa17fe status=succeeded parent=c063e462-572a-48b0-8502-9235ece9ee90
improvement #4 run=b47a22e5-5131-4127-977b-6295e7aa17fe diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 7873fe5a-58ba-4737-b86a-9f2600969391 status=succeeded parent=b47a22e5-5131-4127-977b-6295e7aa17fe

exit=0
````

## Child runs

Parent/child chain.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
7873fe5a-58ba-4737-b86a-9f2600969391   agentic          succeeded  3        2026-11-04T20:58:41.747Z
b47a22e5-5131-4127-977b-6295e7aa17fe   agentic          succeeded  3        2026-11-04T20:58:41.701Z
c063e462-572a-48b0-8502-9235ece9ee90   agentic          succeeded  3        2026-11-04T20:58:41.630Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=c063e462-572a-48b0-8502-9235ece9ee90: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````

## Gateways: run

XOR split/join with skipped branches.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite --cue cue run examples/gateway.cue

run=8ddc26f1-d35b-4137-b2da-da26a53fe4e1 workflow=release-decision status=succeeded durationMs=17
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite status 8ddc26f1-d35b-4137-b2da-da26a53fe4e1

run=8ddc26f1-d35b-4137-b2da-da26a53fe4e1 workflow=release-decision status=succeeded resumeIndex=6
source=./examples/gateway.cue
created=2026-11-04T20:58:41.793Z updated=2026-11-04T20:58:41.811Z
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite trials 8ddc26f1-d35b-4137-b2da-da26a53fe4e1

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
test                 0     0      succeeded 0.950      4        
deployFast           0     0      succeeded 1.000      4        
notify               0     0      succeeded 1.000      4        

exit=0
````

## Gateways: audit stream

gateway.evaluated/joined and step.skipped events.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite logs 8ddc26f1-d35b-4137-b2da-da26a53fe4e1

0 2026-11-04T20:58:41.793Z run.started step=- {"params":{"quality":"fast"},"stateHash":"dd1813fe95a22becf3abd88f8605a2ee9365fb05baa8761039b3dfb4ce1fda12","workflow":"release-decision"}
1 2026-11-04T20:58:41.799Z step.succeeded step=test {"attempts":1,"error":null,"outputHash":"b49800e7e1ff4e636a523d8c8d15acdb0a27d35a871987f8a8a0bb774a1d0540","round":0,"score":0.95}
2 2026-11-04T20:58:41.799Z state.committed step=- {"resumeIndex":1,"stateHash":"b8373eac993c556d556e962502365dd01d5bae2487821ce97cb38d92174c5cc9"}
3 2026-11-04T20:58:41.800Z gateway.evaluated step=route {"chosen":["deployFast"],"default":"deploySlow","evaluated":{"deployFast":{"condition":"steps.test.score >= 0.9 && params.quality == \"fast\"","result":true},"deploySlow":{"default":true,"result":true}},"gateway":"route","round":0,"split":"xor","usedDefault":false}
4 2026-11-04T20:58:41.800Z state.committed step=- {"resumeIndex":2,"stateHash":"b7e0f38ecfd991fa9bb3f9b5544f2f930e115b259b0b8264e841f2509a15665c"}
5 2026-11-04T20:58:41.804Z step.succeeded step=deployFast {"attempts":1,"error":null,"outputHash":"ee0d753a9d044b6f11e644eec8d280f6a9f6231bf623315e23bc0766f8764e14","round":0,"score":1.0}
6 2026-11-04T20:58:41.804Z step.skipped step=deploySlow {"branch":"deploySlow","gateway":"route","reason":"dead_path","round":0,"step":"deploySlow"}
7 2026-11-04T20:58:41.804Z state.committed step=- {"resumeIndex":4,"stateHash":"3b9a932248faac022080db56e8efeb1b552e63cba6d15c48d4a5ae1b920f3525"}
8 2026-11-04T20:58:41.805Z gateway.joined step=merge {"active":["deployFast"],"dead":["deploySlow"],"decision":"taken","gateway":"merge","incoming":["deployFast","deploySlow"],"join":"or","round":0}
9 2026-11-04T20:58:41.805Z state.committed step=- {"resumeIndex":5,"stateHash":"26eebd9a8907aaad70b8fb87c45f9dff7ed43868a7bb64cd9c9c75556f96e927"}
10 2026-11-04T20:58:41.810Z step.succeeded step=notify {"attempts":1,"error":null,"outputHash":"25a50fab20e90ead1f539bad84b2e17ef771f4716ae4aa99ace3c914c70b51e4","round":0,"score":1.0}
11 2026-11-04T20:58:41.810Z state.committed step=- {"resumeIndex":6,"stateHash":"fe7d8dd5d8389f2287c6f1097d2415bd2926a6ba4c2f10708a2cf81a12604f10"}
12 2026-11-04T20:58:41.811Z run.succeeded step=- {"resumeIndex":6}
13 2026-11-04T20:58:41.811Z state.committed step=- {"resumeIndex":6,"stateHash":"fe7d8dd5d8389f2287c6f1097d2415bd2926a6ba4c2f10708a2cf81a12604f10"}

exit=0
````

## Gateways: audit verification

Hash chain and state digest with gateway routing.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite audit verify 8ddc26f1-d35b-4137-b2da-da26a53fe4e1

OK run=8ddc26f1-d35b-4137-b2da-da26a53fe4e1 events=14 head=23192d0ad31dd79c5fd098664a5bd857f0c62828ac1c60182c2a9bd9c7d1e309

exit=0
````

## Gateways: metrics

steps_skipped / gateways_evaluated in Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite metrics --run 8ddc26f1-d35b-4137-b2da-da26a53fe4e1 --format prometheus

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
