# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. Paths normalized; output/hashes/exit codes unchanged.

## Environment

Toolchain and pinning.

````text
ostinato 0.41.2
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
     Running unittests src/main.rs (<target>/debug/deps/ostinato-176902d5e6e39a6b)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (<target>/debug/deps/conformance-64cff948d4626998)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.78s

     Running tests/e2e.rs (<target>/debug/deps/e2e-3ae036db0d11a818)

running 109 tests
test bare_open_list_needs_and_params_compile ... ok
test duplicate_and_nested_tags_follow_cue ... ok
test agents_and_artifacts_error_on_unknown_runs ... ok
test background_grandchild_does_not_extend_a_successful_step ... ok
test acyclic_runs_do_not_emit_cycle_events ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test cleared_integrity_hashes_are_not_silently_repaired ... ok
test audit_verification_detects_tampering ... ok
test bad_template_is_a_durable_failure ... ok
test gateway_audit_detects_a_tampered_skipped_row ... ok
test definition_backed_tags_work_in_both_evaluators ... ok
test gateway_and_fan_out_and_fan_in_wait_for_both_branches ... ok
test gateway_audit_verifies_and_detects_tampered_gateway_output ... ok
test deleting_a_trace_row_is_detected_from_step_outputs ... ok
test gateway_graph_dot_and_json_expose_routing_metadata ... ok
test gateway_expression_join_runs_when_the_expression_is_true ... ok
test gateway_cycle_dead_carry_exits_and_skips_the_tail ... ok
test agent_trace_tampering_is_detected_by_audit_verify ... ok
test filtered_agents_verify_still_checks_the_whole_run ... ok
test gateway_cycle_runs_rounds_and_audits ... ok
test gateway_failure_is_recorded_and_keeps_resume_index_at_the_gateway_group ... ok
test gateway_cycle_round_tampering_fails_audit_verify ... ok
test hidden_metadata_is_allowed_throughout_the_workflow_schema ... ok
test gateway_static_analysis_warns_and_strict_fails ... ok
test agent_definitions_produce_verifiable_traces_and_artifacts ... ok
test cycles_stop_when_until_becomes_true ... ok
test gateway_cycle_with_external_entry_survives_round_one ... ok
test gateway_n_of_partial_quorum_fails_the_run ... ok
test let_and_alias_backed_params_tags_work ... ok
test gateway_n_of_join_runs_when_quorum_is_met ... ok
test gateway_dead_path_propagates_and_records_skips ... ok
test nested_definition_tags_inject_their_declaration ... ok
test gateway_or_runs_every_true_branch ... ok
test nested_tag_aliases_resolve_at_their_written_scope ... ok
test gateway_xor_default_branch_runs_when_no_condition_matches ... ok
test native_evaluator_supports_the_offline_subset ... ok
test non_struct_params_are_rejected ... ok
test gateway_routes_on_continue_on_error_status ... ok
test nested_tags_cover_definitions_hidden_quoted_and_shorthand ... ok
test nested_tags_work_through_definitions_and_reject_bad_types ... ok
test package_underscore_files_are_file_local ... ok
test nested_tags_follow_label_identity_depth_and_expression_bases ... ok
test package_less_cue_file_is_loaded_by_filename ... ok
test gateway_resume_after_split_does_not_re_evaluate_or_run_dead_branch ... ok
test quoted_labels_do_not_leak_evaluator_state_to_stderr ... ok
test leaf_tag_injection_preserves_lexical_scopes ... ok
test params_siblings_resolve_and_true_cycles_fail_cleanly ... ok
test recursive_definitions_do_not_overflow_tag_collection ... ok
test multiline_and_interpolated_bytes_match_both_evaluators ... ok
test legacy_run_without_gateways_still_resumes_and_verifies ... ok
test nested_tags_inject_nested_parameters ... ok
test gateway_xor_routes_one_branch_and_skips_the_other ... ok
test read_only_commands_do_not_require_cue ... ok
test recomputed_state_hash_is_still_rejected_by_the_chain_anchor ... ok
test selector_tags_cover_leaves_duplicates_and_shadowed_definitions ... ok
test selector_tags_follow_definitions_duplicates_and_scope ... ok
test resume_refuses_a_recomputed_digest_that_is_not_anchored ... ok
test split_and_duplicate_definition_tags_work ... ok
test recursive_definitions_materialize_in_both_evaluators ... ok
test tag_attributes_drive_parameter_injection_types ... ok
test tag_identity_handles_shadowing_selectors_and_disjunctions ... ok
test split_package_and_sibling_conflicts_follow_cue_semantics ... ok
test official_cue_runs_and_audit_verifies ... ok
test tag_names_and_value_grammar_follow_cue ... ok
test tag_number_grammar_and_parenthesized_params ... ok
test status_row_tampering_is_detected_by_the_lifecycle_invariant ... ok
test state_tampering_is_detected_and_resume_refuses_it ... ok
test tag_literal_grammar_and_strict_types ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test tool_command_handles_large_stdin_and_capture ... ok
test official_cue_evaluator_resolves_agent_definitions ... ok
test jsonl_provider_output_drives_until_conditions ... ok
test agent_retries_keep_one_trace_per_attempt ... ok
test tool_command_nested_output_ignores_import_concrete_values ... ok
test cycle_rounds_each_get_a_trace ... ok
test resume_extraction_namespaces_replayed_traces_by_id ... ok
test resume_replays_a_group_without_trace_key_collisions ... ok
test tool_command_let_selector_injection_is_an_ostinato_extension ... ok
test tool_command_relative_paths_use_invocation_cwd ... ok
test recursive_improvement_creates_child_runs ... ok
test tool_command_resolves_relative_dir_from_package ... ok
test tool_command_print_and_file_tasks_match_cue ... ok
test tag_placement_violations_match_both_evaluators ... ok
test tool_command_nested_task_outputs_match_cue ... ok
test tool_command_unknown_tag_matches_cue ... ok
test tool_command_referenced_and_unified_commands_match_cue ... ok
test tool_files_stay_excluded_from_normal_workflow_loading ... ok
test exec_timeout_kills_the_process_group ... ok
test unknown_fields_are_rejected ... ok
test truncation_blocks_resume_and_appends ... ok
test validate_tolerates_unused_incomplete_module_fields ... ok
test tool_command_package_after_multi_task_matches_cue ... ok
test tool_command_spawn_error_messages_match_cue ... ok
test tool_command_rejects_conflicting_defaults ... ok
test unused_imports_match_cue ... ok
test tool_command_module_task_exports_match_cue ... ok
test tool_command_selector_commands_match_cue ... ok
test tool_command_let_task_outputs_match_cue ... ok
test tool_command_alias_target_plain_reads_match_cue ... ok
test worktree_isolation_keeps_the_checkout_clean_and_captures_changes ... ok
test tool_command_module_alias_plain_reads_match_cue ... ok
test tool_command_nontask_after_operands_match_cue ... ok
test tool_command_second_hop_alias_chains_match_cue ... ok
test tool_command_package_after_schedule_matches_cue ... ok
test deep_recursive_values_do_not_exhaust_the_stack ... ok
test tool_command_let_and_definition_output_reads_match_cue ... ok
test tool_commands_match_cue ... ok
run=5928ee08-91bc-442e-aa7f-185e2113a01d workflow=cancel status=cancelled durationMs=4025
STEP                     STATUS     VISITS ATTEMPTS
test cancel_stops_a_running_run_and_blocks_resume ... ok

test result: ok. 109 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.10s

     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-eaf5ae18e7aeab35)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.48s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_adapters-0b4df052ebea86a7)

running 8 tests
test process::tests::explicit_command_and_delivery_override_the_provider ... ok
test loaders::tests::parses_release_pre_release_and_build_metadata ... ok
test process::tests::missing_command_provider_is_a_configuration_error ... ok
test process::tests::arg_delivery_rejects_option_like_prompts ... ok
test process::tests::shell_commands_force_stdin_and_reject_arg_delivery ... ok
test process::tests::provider_argv_templates_are_stable ... ok
test process::timeout_tests::timed_out_agents_still_capture_output ... ok
test process::timeout_tests::escaped_descendants_do_not_erase_partial_output ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s

     Running tests/store.rs (<target>/debug/deps/store-3577a2be2de06fda)

running 10 tests
test create_run_with_event_writes_the_start_anchor_atomically ... ok
test sqlite_store_roundtrips_step_rounds ... ok
test resume_run_is_atomic_and_refuses_terminal_runs ... ok
test database_and_wal_files_are_private ... ok
test deleting_an_agent_event_breaks_trace_anchoring ... ok
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok
test agent_traces_roundtrip_and_verify ... ok
test opening_a_v0_database_migrates_steps_round_and_backfills_integrity ... ok
test reopened_store_does_not_repair_blank_integrity_hashes ... ok
test agent_trace_and_artifact_tampering_are_detected ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

     Running tests/workspace.rs (<target>/debug/deps/workspace-087e32d2ec0cb383)

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

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_application-8f31ca21457aa109)

running 22 tests
test activation::tests::and_split_selects_all_branches ... ok
test activation::tests::condition_evaluation_error_is_a_gateway_error ... ok
test activation::tests::normal_step_with_dead_input_is_dead ... ok
test activation::tests::missing_predecessor_is_invalid_state ... ok
test activation::tests::or_join_needs_one_active_input ... ok
test activation::tests::legacy_multiple_needs_keep_and_semantics ... ok
test activation::tests::and_join_requires_every_active_input ... ok
test activation::tests::or_no_true_no_default_fails ... ok
test activation::tests::or_none_uses_default ... ok
test activation::tests::or_selects_every_true_branch ... ok
test activation::tests::n_of_requires_quorum_and_fails_between_zero_and_n ... ok
test activation::tests::expression_join_evaluates_edges ... ok
test activation::tests::xor_join_one_active_is_active ... ok
test activation::tests::split_gateway_branch_choice_controls_edge_state ... ok
test activation::tests::round_zero_carry_is_absent_and_root_entry_starts ... ok
test activation::tests::round_one_carry_reads_previous_round_and_rejects_stale_rows ... ok
test activation::tests::xor_join_two_active_is_ambiguous ... ok
test activation::tests::xor_join_zero_active_is_dead ... ok
test activation::tests::xor_no_true_no_default_fails ... ok
test activation::tests::xor_no_true_uses_default ... ok
test activation::tests::xor_two_true_is_ambiguous ... ok
test activation::tests::xor_unique_true_selects_one ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_domain-3c20564cf1ec197f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/analysis.rs (<target>/debug/deps/analysis-0b4da257cc8362fe)

running 9 tests
test missing_default_is_a_note ... ok
test xor_join_over_non_exclusive_inputs_is_reported ... ok
test n_of_one_over_exclusive_branches_is_reachable ... ok
test branch_that_does_not_reach_a_join_is_reported ... ok
test shared_branch_target_is_reported ... ok
test nested_xor_exclusivity_is_understood ... ok
test xor_branches_feeding_an_xor_join_are_valid ... ok
test xor_branches_feeding_an_and_join_are_reported ... ok
test sibling_branches_with_separate_joins_are_reported ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (<target>/debug/deps/domain-8c9397daf0f27d36)

running 22 tests
test branch_target_with_needs_is_rejected ... ok
test agent_trace_hash_is_deterministic_and_content_sensitive ... ok
test cycles_require_an_explicit_policy ... ok
test audit_chain_detects_tampering ... ok
test invalid_default_is_rejected ... ok
test implicit_join_with_gateways_is_rejected ... ok
test cycles_expand_into_bounded_rounds ... ok
test legacy_step_spec_deserializes_without_gateway_and_reserializes_identically ... ok
test independent_steps_share_a_parallel_group ... ok
test gateway_outside_a_legacy_cycle_is_allowed ... ok
test new_event_helper_is_ergonomic ... ok
test legacy_plan_shape_is_unchanged_without_gateways ... ok
test gateway_cycle_plan_marks_round_scope_and_carry_edges ... ok
test gateway_plan_orders_branches_and_joins ... ok
test join_with_branches_is_rejected ... ok
test n_of_and_expression_joins_are_materialised ... ok
test externally_entered_gateway_cycle_keeps_the_external_edge_round_agnostic ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test legacy_state_digest_is_stable_across_the_gateway_change ... ok
test legacy_agent_specs_keep_their_state_digest_bytes ... ok
test split_and_join_together_is_rejected ... ok
test unknown_branch_target_is_rejected ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (<target>/debug/deps/ostinato_lang-080e3de7f6ac4aa9)

running 3 tests
test gotime::tests::formats_durations ... ok
test gotime::tests::formats_go_layouts ... ok
test gotime::tests::parses_go_layouts ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/agents.rs (<target>/debug/deps/agents-69d920d477592c66)

running 6 tests
test unreferenced_definitions_are_still_validated ... ok
test definition_prompt_and_string_shorthand_are_supported ... ok
test string_command_is_shell_but_argv_is_preserved ... ok
test agent_spec_serialization_stays_compatible_for_inline_steps ... ok
test definitions_are_merged_with_step_overrides ... ok
test invalid_agent_schemas_are_rejected ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (<target>/debug/deps/lang-5f901e42c989674d)

running 246 tests
test aliases_bind_field_values ... ok
test aliases_can_bind_definitions_and_dynamic_labels ... ok
test arithmetic_uses_a_disjunction_default ... ok
test aliases_to_control_prefix_quoted_labels_resolve ... ok
test attributes_parse_in_every_cue_position ... ok
test acyclic_let_chains_are_accepted ... ok
test aliases_and_lets_conflict_with_import_names ... ok
test aliases_work_in_definitions_and_nested_structs ... ok
test absolute_params_paths_do_not_shadow_into_nested_groups ... ok
test bare_open_lists_behave_as_empty_lists ... ok
test aliases_conflict_with_comprehension_bindings ... ok
test bytes_literals_use_go_escape_rules ... ok
test alias_scope_conflicts_and_duplicates_are_rejected ... ok
test arbitrary_precision_integer_literals_keep_exact_values ... ok
test aliases_used_only_in_optional_definition_or_pattern_are_referenced ... ok
test aliases_to_quoted_hash_labels_resolve_the_regular_field ... ok
test bounds_narrow_concrete_values_and_types ... ok
test bound_disjunctions_and_defaults_are_lattice_aware ... ok
test absolute_params_references_inside_generated_groups_resolve ... ok
test chained_dynamic_groups_resolve_sibling_values ... ok
test bytes_interpolation_formats_scalars_and_rejects_incomplete ... ok
test closed_definitions_reject_extra_fields_and_open_definitions_allow_them ... ok
test closed_definitions_stay_closed_through_params ... ok
test alias_shadowing_keeps_dead_bodies_dead ... ok
test bounds_reject_out_of_range_values ... ok
test closed_definitions_are_checked_inside_referenced_bodies ... ok
test closed_definitions_apply_to_nested_dynamic_and_let_params ... ok
test closedness_through_let_mediated_definition_bodies ... ok
test comprehension_clause_sequences_are_supported ... ok
test comprehension_bodies_keep_quoted_hash_fields_next_to_definitions ... ok
test bytes_literal_field_labels_match_string_labels ... ok
test big_integer_bounds_and_builtins_match_cue ... ok
test comprehension_labels_use_displayed_names_for_control_prefixes ... ok
test branch_condition_scope_is_enforced_at_compile_time ... ok
test compiles_workflow_and_resolves_parameters ... ok
test cross_conditional_params_groups_share_materialized_values ... ok
test composed_definitions_keep_referenced_closedness ... ok
test closed_definition_intersections_check_every_operand ... ok
test control_prefix_labels_match_patterns ... ok
test decimal_arithmetic_is_exact ... ok
test defaulted_disjunctions_work_in_select_index_and_for ... ok
test comprehension_generated_definitions_are_selectable_and_hidden ... ok
test comprehension_generated_declarations_merge_like_cue ... ok
test definition_optional_conflicts_error_only_when_materialized ... ok
test definitions_can_be_used_in_conditions_and_selectors ... ok
test derived_definitions_keep_referenced_closedness ... ok
test definitions_behave_like_structs_in_comprehensions_and_equality ... ok
test dependent_parameter_overrides_are_order_independent ... ok
test definition_selectors_and_quoted_hash_fields_in_params_paths ... ok
test definitions_preserve_optional_constraints_while_open ... ok
test duplicate_disjunction_defaults_collapse ... ok
test bounds_validate_operands_types_and_intervals ... ok
test disjunction_defaults_narrow_and_export ... ok
test definition_disjunctions_keep_alternatives ... ok
test comprehension_generated_definitions_respect_caches_and_shadowing ... ok
test evaluates_conditions ... ok
test definition_backed_params_see_overrides_in_dependents ... ok
test definition_optional_constraint_applies_to_required_fields_in_both_orders ... ok
test disjunction_drops_conflicting_alternatives_but_not_bad_references ... ok
test division_by_zero_is_an_error_not_a_panic ... ok
test disjunction_intersection_and_narrowing_match_cue ... ok
test dynamic_index_then_selector_resolves_params_paths ... ok
test compiles_every_split_and_join_mode ... ok
test forward_let_references_are_lazy ... ok
test dynamic_intermediate_and_repeated_segments_resolve ... ok
test disjunction_dedupe_preserves_definition_metadata ... ok
test definitions_do_not_admit_regular_fields_of_the_same_label ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test escaped_patterns_and_quoted_labels_decode_correctly ... ok
test gateway_compiles_identically_through_value_and_json ... ok
test gateway_condition_scope_accepts_transitive_predecessors_and_ignores_string_literals ... ok
test gateway_conditions_and_step_status_are_accepted ... ok
test escaped_slash_case_mapping_and_decimal_ranges ... ok
test gateway_definition_backed_branches_compile_identically ... ok
test generated_hidden_optionals_in_definitions_are_optional ... ok
test gateway_cycles_and_implicit_joins_are_rejected_at_compile_time ... ok
test dead_comprehension_bodies_do_not_create_required_cycles ... ok
test gateway_n_of_and_expression_modes_compile ... ok
test generated_hidden_optionals_do_not_hide_visible_sentinel_labels ... ok
test generated_optional_conflicts_when_required_field_comes_later ... ok
test generated_optional_constraint_conflicts_with_a_required_field ... ok
test generated_optional_conflicts_with_required_dynamic_label ... ok
test generated_only_optional_fields_are_not_materialized ... ok
test generated_optional_constraint_applies_to_a_required_field ... ok
test equality_requires_concrete_operands ... ok
test generated_root_optionals_do_not_hide_definitions ... ok
test gateway_n_of_duplicate_needs_and_non_injective_labels_are_rejected ... ok
test hidden_fields_are_invisible_to_value_operations ... ok
test generated_quoted_optionals_do_not_touch_materialized_definitions ... ok
test generated_quoted_optionals_target_the_regular_field ... ok
test generated_hidden_params_are_not_reachable_through_quoted_selectors ... ok
test gateway_n_of_and_expression_validation_is_path_qualified ... ok
test generated_quoted_hash_and_control_labels_are_selectable ... ok
test generated_optional_unifies_with_required_dynamic_label ... ok
test identifier_select_does_not_reach_quoted_hash_fields ... ok
test imported_packages_drop_package_hidden_fields ... ok
test interpolation_rejects_incomplete_disjunctions ... ok
test generated_params_fields_respect_hidden_quoting_namespaces ... ok
test identifier_selectors_prefer_hidden_over_quoted_twins ... ok
test imported_packages_keep_quoted_hash_fields_next_to_definitions ... ok
test hidden_fields_are_distinct_from_quoted_underscore_fields ... ok
test invalid_aliases_are_rejected ... ok
test inner_alias_shadows_an_enclosing_let ... ok
test imported_hidden_fields_do_not_leak_into_the_importer ... ok
test inner_lets_can_shadow_outer_lets_and_aliases ... ok
test invalid_regex_fails_even_when_optional_value_conflicts ... ok
test invalid_regex_on_optional_fields_is_eager ... ok
test int_and_float_do_not_unify ... ok
test list_comprehension_struct_bodies_still_support_patterns ... ok
test invalid_until_is_rejected_at_validation_time ... ok
test hidden_fields_are_evaluated_selected_and_conflict_checked ... ok
test let_cycles_are_rejected ... ok
test list_comprehension_bodies_can_start_with_brackets ... ok
test missing_field_separators_are_rejected ... ok
test list_comprehensions_are_supported ... ok
test list_comprehensions_support_struct_bodies ... ok
test let_shadowing_does_not_require_an_outer_reference ... ok
test multiline_bytes_escaped_backslash_before_paren_is_literal ... ok
test materialized_sentinel_fields_stay_required_and_conflict_checked ... ok
test math_rounding_is_decimal_exact ... ok
test let_cycles_through_required_definitions_are_rejected ... ok
test integer_bound_intervals_respect_discreteness ... ok
test materialized_hidden_fields_do_not_satisfy_quoted_optionals ... ok
test multi_expression_definitions_close_after_composition ... ok
test multiline_bytes_dedent_handles_cr_and_interpolation_lines ... ok
test let_conflicts_and_unreferenced_lets_are_rejected ... ok
test definitions_can_select_their_own_fields ... ok
test multiline_bytes_literals_dedent_and_interpolate ... ok
test native_defaults_work_in_multibranch_disjunctions ... ok
test multiple_matching_patterns_merge_before_closing ... ok
test named_definition_flattens_optional_field_materialization ... ok
test math_integral_results_keep_the_cue_kind ... ok
test native_skips_attributes_with_equals_and_omits_optional_fields ... ok
test nested_definitions_are_selectable ... ok
test native_overrides_apply_before_top_level_comprehensions ... ok
test native_parameter_overrides_are_injected_before_interpolation ... ok
test nested_definitions_are_closed_unless_opened ... ok
test nested_disjunction_defaults_respect_direct_precedence ... ok
test nested_params_groups_see_lexical_siblings_through_paths ... ok
test nested_definitions_are_not_exported_as_fields ... ok
test nested_params_groups_see_outer_overrides ... ok
test open_list_element_types_are_enforced ... ok
test nested_patterns_constrain_nested_fields ... ok
test numeric_equality_and_float_export_match_cue ... ok
test nested_multiline_literals_inside_interpolation_match_cue ... ok
test open_list_prefix_unifies_with_a_concrete_list ... ok
test multiline_strings_use_cue_dedent_and_cr_rules ... ok
test nested_params_groups_and_generated_chains_avoid_false_cycles ... ok
test open_list_prefix_values_export_their_prefix ... ok
test gateway_payload_rules_are_rejected_with_full_paths ... ok
test optional_fields_cannot_be_selected_from_a_definition ... ok
test optional_fields_cannot_be_referenced_by_bare_name ... ok
test optional_definition_fields_are_not_materialized ... ok
test nested_struct_unification_is_closed_after_composition ... ok
test optional_fields_are_pattern_constrained_when_materialized ... ok
test numeric_equality_is_scalar_only ... ok
test optional_nested_definitions_are_not_selectable ... ok
test optional_intermediate_params_fields_are_rejected ... ok
test non_struct_definition_values_close_nested_structs ... ok
test open_list_prefixes_work_as_lists ... ok
test go_time_layouts_and_helpers_match_cue ... ok
test optional_value_conflicts_are_still_deferred ... ok
test optional_pattern_conflicts_are_deferred_until_materialized ... ok
test optional_values_get_patterns_before_closing ... ok
test parameter_overrides_narrow_a_default_free_disjunction ... ok
test gateway_additional_validation_rules_are_path_qualified ... ok
test parameter_overrides_are_constraints_and_dependent_fields_see_them ... ok
test parameters_can_be_backed_by_a_definition_or_split_struct ... ok
test parameter_overrides_respect_fixed_values_and_types ... ok
test open_list_prefixes_align_and_enforce_element_constraints ... ok
test optional_fields_do_not_leak_through_unification_and_vars_shadow ... ok
test overrides_are_visible_to_params_comprehensions ... ok
test overrides_keep_generated_optional_constraints ... ok
test params_optional_and_shadowing_rules_hold ... ok
test params_cache_distinguishes_dotted_labels_from_paths ... ok
test parenthesized_definition_literal_keeps_optional_constraints ... ok
test optional_open_metadata_on_struct_literals_matches_cue ... ok
test parenthesized_dynamic_labels_and_if_clause_sequences_work ... ok
test params_items_can_depend_on_later_generated_siblings ... ok
test patterns_apply_to_declared_fields ... ok
test pattern_values_can_be_disjunctions ... ok
test live_struct_sources_and_constant_conditions_keep_cycles ... ok
test params_dynamic_fields_hidden_definitions_and_quoted_selectors ... ok
test params_comprehensions_can_read_siblings_without_false_cycles ... ok
test params_dependency_chains_resolve_in_comprehensions ... ok
test quoted_hash_labels_are_regular_fields ... ok
test params_siblings_numeric_literals_and_bytes_concat ... ok
test parenthesized_params_keep_overrides ... ok
test params_paths_resolve_against_the_params_root ... ok
test params_resolution_handles_nested_groups_lets_and_fields ... ok
test quoted_labels_do_not_conflict_with_aliases ... ok
test quoted_hash_labels_are_selectable_from_struct_values ... ok
test promoted_optional_fields_are_closed_by_other_patterns ... ok
test params_index_paths_resolve_through_nested_groups ... ok
test quoted_parameter_names_use_identifier_values ... ok
test quoted_hash_labels_are_distinct_from_definitions ... ok
test nested_comprehension_bodies_shadow_in_static_liveness ... ok
test recursive_nested_definitions_do_not_overflow ... ok
test renders_templates ... ok
test quoted_labels_are_not_identifier_bindings ... ok
test pattern_value_structs_are_closed_for_external_fields ... ok
test patterns_allow_and_constrain_extra_fields ... ok
test quoted_and_index_selectors_do_not_reach_hidden_fields ... ok
test retry_int_shorthand_respects_the_cap ... ok
test rejects_missing_required_parameter ... ok
test recursive_definitions_apply_nested_constraints ... ok
test self_selection_same_body_comprehension_is_a_documented_limit ... ok
test quoted_params_lookup_does_not_return_cached_definitions ... ok
test recursive_definitions_materialize_finite_values ... ok
test sentinel_prefixed_quoted_labels_select_their_escaped_storage ... ok
test root_params_patterns_constrain_fields ... ok
test step_level_unknown_fields_are_rejected ... ok
test regexp_quote_find_and_re2_classes_match_cue ... ok
test regexp_uses_ascii_classes_and_repeat_limits ... ok
test re2_quantifier_grammar_matches_cue ... ok
test string_escapes_are_decoded_and_invalid_ones_rejected ... ok
test string_and_bytes_repetition_match_cue ... ok
test string_index_addresses_regular_fields_only ... ok
test scalar_unification_is_symmetric_and_strict ... ok
test underscore_alias_is_rejected ... ok
test top_in_disjunctions_is_preserved ... ok
test recursive_definitions_fill_defaults_at_every_depth ... ok
test narrowed_defaults_recompute_dependent_fields ... ok
test unicode_string_builtins_match_go_semantics ... ok
test unicode_surrogates_and_string_escape_edges ... ok
test unknown_workflow_fields_are_rejected ... ok
test unreferenced_alias_inside_definition_is_rejected ... ok
test struct_string_indexes_respect_sentinel_namespaces ... ok
test versioned_imports_use_the_unversioned_alias ... ok
test time_garbage_and_duration_fraction_edges ... ok
test time_fractional_layouts_and_duration_edges ... ok
test unification_sources_are_live_when_either_side_is_live ... ok
test unresolved_and_ambiguous_disjunctions_are_not_concrete ... ok
test regex_class_complements_and_repeat_scanner_are_re2_compatible ... ok
test regexp_math_and_time_builtins_match_cue ... ok
test time_helpers_handle_fractions_signs_and_nanos ... ok
test until_accepts_nested_json_provider_and_hyphenated_steps ... ok
test struct_literal_shadowing_keeps_dead_bodies_dead ... ok
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
     Running tests/conformance.rs (<target>/debug/deps/conformance-64cff948d4626998)

running 2 tests
test rust_and_official_cue_reject_invalid_corpus ... ok
test rust_matches_official_cue_on_the_conformance_corpus ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.81s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running tests/tool_conformance.rs (<target>/debug/deps/tool_conformance-eaf5ae18e7aeab35)

running 1 test
test tool_commands_match_cue_on_the_conformance_corpus ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.72s


exit=0
````

## Deterministic DAG (Rust default)

Dependency ordering and typed parameter.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=79bf5e99-da3a-4433-8699-b1aa4f8cb4ff workflow=hello status=succeeded durationMs=10
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

run=745ca40c-52a1-472f-8fd2-520ba2764649 workflow=retry status=succeeded durationMs=324
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

Review/refine until score >= 0.8.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2 workflow=cycle status=succeeded durationMs=87
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Run status

Visits/attempts.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite status 27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2

run=27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2 workflow=cycle status=succeeded resumeIndex=11
source=./examples/cycle.cue
created=2026-11-05T00:13:58.575Z updated=2026-11-05T00:13:58.663Z
STEP                     STATUS     VISITS ATTEMPTS ROUND 
refine                   succeeded  3      1        2     
review                   succeeded  3      1        2     
setup                    succeeded  1      1        0     

exit=0
````

## Audit stream

state.committed, cycle.completed, checkpoints.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite logs 27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2

0 2026-11-05T00:13:58.575Z run.started step=- {"params":{},"stateHash":"167fe618f1b84d5b96ad0cafcad28236ae4e12f80f5b8d804ce39447ed3c9ae6","workflow":"cycle"}
1 2026-11-05T00:13:58.594Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-05T00:13:58.594Z state.committed step=- {"resumeIndex":1,"stateHash":"e07d89dfbd958947b291d9d25eda3ec0de97a436a43361ff2eb3bed53aa0244c"}
3 2026-11-05T00:13:58.611Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
4 2026-11-05T00:13:58.611Z state.committed step=- {"resumeIndex":2,"stateHash":"83d915fc6b37fd489f6b11743cee571b2c4172f5810c0ceecb1a3eed6f82a488"}
5 2026-11-05T00:13:58.619Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
6 2026-11-05T00:13:58.619Z state.committed step=- {"resumeIndex":3,"stateHash":"8eaf98589f23dc10d6f839c92916f1345fbf44a13fad710e9ef20401404b5e35"}
7 2026-11-05T00:13:58.632Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
8 2026-11-05T00:13:58.632Z state.committed step=- {"resumeIndex":4,"stateHash":"bbdda11874ca2b882355f900cdb4bedd0b8cc12298be946b95838ad21b23b3a7"}
9 2026-11-05T00:13:58.641Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
10 2026-11-05T00:13:58.641Z state.committed step=- {"resumeIndex":5,"stateHash":"e5d87774f03ec40fea870b480c00d54f909c7ef68655557c5e8496199de12823"}
11 2026-11-05T00:13:58.656Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
12 2026-11-05T00:13:58.656Z state.committed step=- {"resumeIndex":6,"stateHash":"193d51d1a24a0b29285850ef7014da352010470e16268823aa7324d21cae35ee"}
13 2026-11-05T00:13:58.663Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
14 2026-11-05T00:13:58.663Z cycle.completed step=refine {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
15 2026-11-05T00:13:58.663Z state.committed step=- {"resumeIndex":11,"stateHash":"ff5db24e1aea13fca4758103a56dbe800f5a9f3b5a5ceb691d754a9ad59db4b5"}
16 2026-11-05T00:13:58.663Z run.succeeded step=- {"resumeIndex":11}
17 2026-11-05T00:13:58.663Z state.committed step=- {"resumeIndex":11,"stateHash":"ff5db24e1aea13fca4758103a56dbe800f5a9f3b5a5ceb691d754a9ad59db4b5"}

exit=0
````

## Trial log

One row per attempt.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite trials 27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      17       
review               0     0      succeeded 0.300      15       
refine               0     0      succeeded 1.000      7        
review               1     0      succeeded 0.600      12       
refine               1     0      succeeded 1.000      7        
review               2     0      succeeded 0.900      13       
refine               2     0      succeeded 1.000      6        

exit=0
````

## Audit verification

Hash chain, state digest, anchor.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite audit verify 27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2

OK run=27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2 events=18 head=120b022f2433e53bf9faf0d715f23a2b3c1a6464d4eaf024b48b2faabb62a1de

exit=0
````

## Metrics

Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/db.sqlite metrics --run 27cc7286-a3a4-49e4-aaa5-0f9c53a9e6a2 --format prometheus

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
ostinato_attempt_duration_ms_avg 11.00

exit=0
````

## Durability: failure

Checkpoint and run.failed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=7c141bda-73f2-4802-9166-a6dec21f88df workflow=resume-evidence status=failed durationMs=11
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

Resume finishes.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite resume 7c141bda-73f2-4802-9166-a6dec21f88df

run=7c141bda-73f2-4802-9166-a6dec21f88df workflow=resume-evidence status=succeeded durationMs=6
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

Completed step not re-executed.

````text
### <target>/debug/ostinato --db <workdir>/resume.sqlite trials 7c141bda-73f2-4802-9166-a6dec21f88df

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      5        
b                    0     0      failed   0.000      5        command exited with 7: 
b                    0     0      succeeded 1.000      5        

exit=0
````

## Agentic workflow

agent + improve + report.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=22490762-1150-49d9-80b6-da0faa64b963 workflow=agentic status=succeeded durationMs=32
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Patched-param child runs.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite improve 22490762-1150-49d9-80b6-da0faa64b963 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=22490762-1150-49d9-80b6-da0faa64b963 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run 0f4f7608-8208-4526-a600-7f2dd5585c20 status=succeeded parent=22490762-1150-49d9-80b6-da0faa64b963
improvement #4 run=0f4f7608-8208-4526-a600-7f2dd5585c20 diagnosis=The plan is sound; keep the scope small and reversible.
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
child run e61109b0-5f98-4658-bcc8-3b3003af986d status=succeeded parent=0f4f7608-8208-4526-a600-7f2dd5585c20

exit=0
````

## Child runs

Parent/child chain.

````text
### <target>/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
e61109b0-5f98-4658-bcc8-3b3003af986d   agentic          succeeded  3        2026-11-05T00:13:58.916Z
0f4f7608-8208-4526-a600-7f2dd5585c20   agentic          succeeded  3        2026-11-05T00:13:58.869Z
22490762-1150-49d9-80b6-da0faa64b963   agentic          succeeded  3        2026-11-05T00:13:58.795Z

exit=0
````

## Tamper detection

Payload mutation fails.

````text
AUDIT FAILED run=22490762-1150-49d9-80b6-da0faa64b963: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````

## Gateways: run

XOR split/join with skipped branches.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite --cue cue run examples/gateway.cue

run=e418bc5d-5dd3-4625-8e7b-4650ef67e318 workflow=release-decision status=succeeded durationMs=18
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite status e418bc5d-5dd3-4625-8e7b-4650ef67e318

run=e418bc5d-5dd3-4625-8e7b-4650ef67e318 workflow=release-decision status=succeeded resumeIndex=6
source=./examples/gateway.cue
created=2026-11-05T00:13:58.967Z updated=2026-11-05T00:13:58.986Z
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
### <target>/debug/ostinato --db <workdir>/gateway.sqlite trials e418bc5d-5dd3-4625-8e7b-4650ef67e318

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
test                 0     0      succeeded 0.950      5        
deployFast           0     0      succeeded 1.000      4        
notify               0     0      succeeded 1.000      4        

exit=0
````

## Gateways: audit stream

gateway.evaluated/joined and step.skipped events.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite logs e418bc5d-5dd3-4625-8e7b-4650ef67e318

0 2026-11-05T00:13:58.967Z run.started step=- {"params":{"quality":"fast"},"stateHash":"6e75fcb7cdc3db885fd38114fe078423b26cd40719b0a166b94eee815fe468e5","workflow":"release-decision"}
1 2026-11-05T00:13:58.974Z step.succeeded step=test {"attempts":1,"error":null,"outputHash":"b49800e7e1ff4e636a523d8c8d15acdb0a27d35a871987f8a8a0bb774a1d0540","round":0,"score":0.95}
2 2026-11-05T00:13:58.974Z state.committed step=- {"resumeIndex":1,"stateHash":"82a0be19fba16c71243c32f93e9c22a45c61908a8a19542bc96b381c408dcb42"}
3 2026-11-05T00:13:58.974Z gateway.evaluated step=route {"chosen":["deployFast"],"default":"deploySlow","evaluated":{"deployFast":{"condition":"steps.test.score >= 0.9 && params.quality == \"fast\"","result":true},"deploySlow":{"default":true,"result":true}},"gateway":"route","round":0,"split":"xor","usedDefault":false}
4 2026-11-05T00:13:58.974Z state.committed step=- {"resumeIndex":2,"stateHash":"c226111a140aa1e058804da1224ef2071601795d8eb9c223d3859a1d3d096858"}
5 2026-11-05T00:13:58.980Z step.succeeded step=deployFast {"attempts":1,"error":null,"outputHash":"ee0d753a9d044b6f11e644eec8d280f6a9f6231bf623315e23bc0766f8764e14","round":0,"score":1.0}
6 2026-11-05T00:13:58.980Z step.skipped step=deploySlow {"branch":"deploySlow","gateway":"route","reason":"dead_path","round":0,"step":"deploySlow"}
7 2026-11-05T00:13:58.980Z state.committed step=- {"resumeIndex":4,"stateHash":"aa526835ebcc228c4230e1314c255ca4a99b39a35008c4c35f74840beb1315d7"}
8 2026-11-05T00:13:58.980Z gateway.joined step=merge {"active":["deployFast"],"dead":["deploySlow"],"decision":"taken","gateway":"merge","incoming":["deployFast","deploySlow"],"join":"or","round":0}
9 2026-11-05T00:13:58.980Z state.committed step=- {"resumeIndex":5,"stateHash":"5fc470f28d29063672e9de5416f030e4378b9df61f095b1ec5abf176e9987a1c"}
10 2026-11-05T00:13:58.986Z step.succeeded step=notify {"attempts":1,"error":null,"outputHash":"25a50fab20e90ead1f539bad84b2e17ef771f4716ae4aa99ace3c914c70b51e4","round":0,"score":1.0}
11 2026-11-05T00:13:58.986Z state.committed step=- {"resumeIndex":6,"stateHash":"95e7450307814214cf84ab334d639d6087f5469354a9eaa7f6b06dc773ff6dd0"}
12 2026-11-05T00:13:58.986Z run.succeeded step=- {"resumeIndex":6}
13 2026-11-05T00:13:58.986Z state.committed step=- {"resumeIndex":6,"stateHash":"95e7450307814214cf84ab334d639d6087f5469354a9eaa7f6b06dc773ff6dd0"}

exit=0
````

## Gateways: audit verification

Hash chain and state digest with gateway routing.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite audit verify e418bc5d-5dd3-4625-8e7b-4650ef67e318

OK run=e418bc5d-5dd3-4625-8e7b-4650ef67e318 events=14 head=f4be0285fffc53e517ec3405dfceab2fb60b3cab9322c22ed5c62ca554347870

exit=0
````

## Gateways: metrics

steps_skipped / gateways_evaluated in Prometheus text.

````text
### <target>/debug/ostinato --db <workdir>/gateway.sqlite metrics --run e418bc5d-5dd3-4625-8e7b-4650ef67e318 --format prometheus

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
ostinato_attempt_duration_ms_avg 4.33

exit=0
````
