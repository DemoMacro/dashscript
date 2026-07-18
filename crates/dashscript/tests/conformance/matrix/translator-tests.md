# DashScript Conformance Matrix

- 201 features: **200** supported, **1** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## arrays

| feature                                                      | status       | detail / note |
| ------------------------------------------------------------ | ------------ | ------------- |
| arrays.translates_array_literal_to_vec_macro                 | 🟢 supported |               |
| arrays.translates_array_index                                | 🟢 supported |               |
| arrays.translates_in_operator_on_array_to_index_bound        | 🟢 supported |               |
| arrays.translates_array_map_to_iter_copied_map_collect       | 🟢 supported |               |
| arrays.translates_array_filter_to_iter_copied_filter_collect | 🟢 supported |               |
| arrays.translates_array_slice_to_index_range_to_vec          | 🟢 supported |               |
| arrays.translates_array_index_of_to_position                 | 🟢 supported |               |
| arrays.translates_array_find_index_to_position               | 🟢 supported |               |
| arrays.translates_array_at_to_signed_runtime_index           | 🟢 supported |               |
| arrays.translates_array_flat_map_to_flat_map_collect         | 🟢 supported |               |
| arrays.translates_array_literal_with_expression_elements     | 🟢 supported |               |
| arrays.translates_array_flat_to_concat                       | 🟢 supported |               |
| arrays.translates_array_last_index_of_to_rposition           | 🟢 supported |               |
| arrays.translates_array_fill_to_vec_fill                     | 🟢 supported |               |
| arrays.translates_array_for_each_to_for_each                 | 🟢 supported |               |
| arrays.translates_array_includes_to_contains                 | 🟢 supported |               |
| arrays.translates_array_find_to_iter_copied_find             | 🟢 supported |               |
| arrays.translates_array_some_every_to_any_all                | 🟢 supported |               |
| arrays.translates_array_join_to_vec_string_join              | 🟢 supported |               |
| arrays.translates_array_reduce_with_seed_to_fold             | 🟢 supported |               |
| arrays.translates_array_reduce_without_seed_to_reduce        | 🟢 supported |               |
| arrays.translates_array_index_assign_to_usize_index          | 🟢 supported |               |
| arrays.translates_array_concat_to_slice_concat               | 🟢 supported |               |
| arrays.translates_array_reverse_to_in_place_reverse          | 🟢 supported |               |
| arrays.translates_array_sort_to_numeric_sort_by              | 🟢 supported |               |
| arrays.translates_array_find_last_to_rev_find                | 🟢 supported |               |
| arrays.translates_array_find_last_index_to_rposition         | 🟢 supported |               |
| arrays.translates_array_reduce_right_to_rev_fold             | 🟢 supported |               |
| arrays.translates_array_to_sorted_to_clone_sort              | 🟢 supported |               |
| arrays.translates_array_to_reversed_to_rev_collect           | 🟢 supported |               |
| arrays.translates_array_to_spliced_to_clone_splice           | 🟢 supported |               |
| arrays.translates_array_with_to_clone_index_assign           | 🟢 supported |               |
| arrays.translates_array_shift_unshift_pop                    | 🟢 supported |               |
| arrays.translates_array_of                                   | 🟢 supported |               |
| arrays.translates_array_is_array_vec_true                    | 🟢 supported |               |
| arrays.translates_array_from_clone                           | 🟢 supported |               |
| arrays.translates_array_from_mapped                          | 🟢 supported |               |
| arrays.translates_array_splice                               | 🟢 supported |               |
| arrays.translates_array_to_string_join                       | 🟢 supported |               |
| arrays.translates_array_copy_within                          | 🟢 supported |               |

## classes

| feature                               | status       | detail / note |
| ------------------------------------- | ------------ | ------------- |
| classes.translates_new_expression     | 🟢 supported |               |
| classes.translates_new_with_arguments | 🟢 supported |               |

## clone_move

| feature                                                         | status       | detail / note |
| --------------------------------------------------------------- | ------------ | ------------- |
| clone_move.translates_unmutated_let_is_plain_let                | 🟢 supported |               |
| clone_move.translates_mutated_let_is_let_mut                    | 🟢 supported |               |
| clone_move.translates_mutated_var_is_let_mut                    | 🟢 supported |               |
| clone_move.translates_mutated_let_by_compound_assign_is_let_mut | 🟢 supported |               |
| clone_move.translates_mutated_vec_by_method_is_let_mut          | 🟢 supported |               |
| clone_move.translates_for_in_to_keys_cloned                     | 🟢 supported |               |

## console

| feature                                     | status       | detail / note |
| ------------------------------------------- | ------------ | ------------- |
| console.translates_multi_arg_console_log    | 🟢 supported |               |
| console.translates_console_warn_to_eprintln | 🟢 supported |               |

## control_flow

| feature                                                | status       | detail / note                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------ | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| control_flow.translates_if_else                        | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_while_with_update              | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_for_of_as_borrow               | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_switch_to_match                | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_do_while                       | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_c_style_for_loop               | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_break_and_continue             | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_if_collection_truthiness       | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_if_option_truthiness           | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_throw_new_error_to_panic       | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_throw_string_to_panic          | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_try_catch_to_catch_unwind      | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_try_finally_runs_after_match   | 🟢 supported |                                                                                                                                                                                                                                                                                        |
| control_flow.translates_try_block_with_return_rejected | 🟡 partial   | src\main.rs:2:5: error: DashScript try blocks cannot contain return/break/continue (control flow cannot cross the catch boundary) src\main.rs:1:11: error[E0308]: mismatched types: expected `f64`, found `()` error: could not compile `probe` (bin "probe") due to 2 previous errors |

## destructuring

| feature                                                          | status       | detail / note |
| ---------------------------------------------------------------- | ------------ | ------------- |
| destructuring.translates_object_destructure_default_to_unwrap_or | 🟢 supported |               |
| destructuring.translates_discriminated_union_switch_destructure  | 🟢 supported |               |
| destructuring.translates_object_destructuring_to_struct_pattern  | 🟢 supported |               |
| destructuring.translates_array_destructuring_to_indexed_lets     | 🟢 supported |               |
| destructuring.translates_array_destructure_rest_to_slice         | 🟢 supported |               |
| destructuring.translates_object_spread_to_struct_update          | 🟢 supported |               |
| destructuring.translates_array_spread_to_slice_concat            | 🟢 supported |               |
| destructuring.translates_array_destructure_skips_holes           | 🟢 supported |               |
| destructuring.translates_object_destructure_rename               | 🟢 supported |               |

## globals

| feature                                                      | status       | detail / note |
| ------------------------------------------------------------ | ------------ | ------------- |
| globals.translates_string_global_to_format                   | 🟢 supported |               |
| globals.translates_parse_int_to_parse_f64                    | 🟢 supported |               |
| globals.translates_parse_int_with_radix_to_from_str_radix    | 🟢 supported |               |
| globals.translates_number_global_string_to_parse             | 🟢 supported |               |
| globals.translates_number_global_string_var_to_parse         | 🟢 supported |               |
| globals.translates_number_global_number_passes_through       | 🟢 supported |               |
| globals.translates_boolean_global_zero_to_false              | 🟢 supported |               |
| globals.translates_boolean_global_nonzero_to_true            | 🟢 supported |               |
| globals.translates_boolean_global_string_literal_to_is_empty | 🟢 supported |               |
| globals.translates_boolean_global_vec_to_is_empty            | 🟢 supported |               |
| globals.translates_boolean_global_number_var_to_ne_zero      | 🟢 supported |               |
| globals.translates_boolean_global_option_to_is_some          | 🟢 supported |               |
| globals.translates_number_static_type_checks                 | 🟢 supported |               |
| globals.translates_number_is_safe_integer                    | 🟢 supported |               |
| globals.translates_number_constants                          | 🟢 supported |               |
| globals.translates_number_parse_float                        | 🟢 supported |               |
| globals.translates_number_parse_int_radix                    | 🟢 supported |               |
| globals.translates_number_to_exponential                     | 🟢 supported |               |
| globals.translates_number_value_of                           | 🟢 supported |               |
| globals.translates_global_is_nan                             | 🟢 supported |               |
| globals.translates_global_is_finite                          | 🟢 supported |               |

## math

| feature                                         | status       | detail / note |
| ----------------------------------------------- | ------------ | ------------- |
| math.translates_math_methods                    | 🟢 supported |               |
| math.translates_math_constants                  | 🟢 supported |               |
| math.translates_math_trig_and_log_methods       | 🟢 supported |               |
| math.translates_math_log_to_ln                  | 🟢 supported |               |
| math.translates_math_atan2_to_atan2             | 🟢 supported |               |
| math.translates_math_hypot_to_pythagoras        | 🟢 supported |               |
| math.translates_math_log1p_to_ln_1p             | 🟢 supported |               |
| math.translates_math_expm1_to_exp_m1            | 🟢 supported |               |
| math.translates_math_clz32_to_leading_zeros     | 🟢 supported |               |
| math.translates_math_fround_to_f32_round_trip   | 🟢 supported |               |
| math.translates_math_imul_to_wrapping_mul       | 🟢 supported |               |
| math.translates_math_sign_to_signum             | 🟢 supported |               |
| math.translates_math_hyperbolic_methods         | 🟢 supported |               |
| math.translates_math_inverse_hyperbolic_methods | 🟢 supported |               |
| math.translates_math_inverse_trig_methods       | 🟢 supported |               |
| math.translates_math_extra_constants            | 🟢 supported |               |
| math.translates_math_rounding_and_root_methods  | 🟢 supported |               |
| math.translates_math_exp_log_trig_methods       | 🟢 supported |               |
| math.translates_math_min_and_e_constant         | 🟢 supported |               |
| math.translates_math_round_avoids_add_half_bug  | 🟢 supported |               |
| math.translates_math_sign_keeps_signed_zero     | 🟢 supported |               |

## narrowing

| feature                                                        | status       | detail / note |
| -------------------------------------------------------------- | ------------ | ------------- |
| narrowing.translates_optional_chain_to_as_ref_map              | 🟢 supported |               |
| narrowing.translates_optional_chain_coalesce_to_unwrap_or      | 🟢 supported |               |
| narrowing.translates_some_wrapping                             | 🟢 supported |               |
| narrowing.translates_non_null_assertion                        | 🟢 supported |               |
| narrowing.translates_null_equality_to_is_none                  | 🟢 supported |               |
| narrowing.translates_null_inequality_to_is_some                | 🟢 supported |               |
| narrowing.translates_nullish_coalescing_to_unwrap_or_else      | 🟢 supported |               |
| narrowing.translates_logical_or_value_returns_left_when_truthy | 🟢 supported |               |
| narrowing.translates_logical_or_bool_short_circuits            | 🟢 supported |               |
| narrowing.translates_logical_nullish_assign                    | 🟢 supported |               |
| narrowing.translates_logical_or_assign                         | 🟢 supported |               |

## number_methods

| feature                                                       | status       | detail / note |
| ------------------------------------------------------------- | ------------ | ------------- |
| number_methods.translates_number_to_fixed_to_format_precision | 🟢 supported |               |
| number_methods.translates_number_to_string_radix_hex          | 🟢 supported |               |
| number_methods.translates_number_to_string_radix_binary       | 🟢 supported |               |
| number_methods.translates_number_to_string_no_arg_is_display  | 🟢 supported |               |

## operators

| feature                                              | status       | detail / note |
| ---------------------------------------------------- | ------------ | ------------- |
| operators.translates_arithmetic_and_comparison       | 🟢 supported |               |
| operators.translates_logical_and_unary               | 🟢 supported |               |
| operators.translates_compound_assignment             | 🟢 supported |               |
| operators.translates_template_literal                | 🟢 supported |               |
| operators.translates_ternary_to_if_expression        | 🟢 supported |               |
| operators.translates_length_to_len                   | 🟢 supported |               |
| operators.translates_exponent_operator               | 🟢 supported |               |
| operators.translates_in_operator_to_contains_key     | 🟢 supported |               |
| operators.translates_arrow_function_expression_body  | 🟢 supported |               |
| operators.translates_field_assign_to_field           | 🟢 supported |               |
| operators.translates_bitwise_and_or_xor              | 🟢 supported |               |
| operators.translates_bitwise_shifts                  | 🟢 supported |               |
| operators.translates_bitwise_not                     | 🟢 supported |               |
| operators.translates_bitwise_compound_assign         | 🟢 supported |               |
| operators.translates_comparison_chain_short_circuits | 🟢 supported |               |
| operators.translates_logical_not_short_circuits      | 🟢 supported |               |

## strings

| feature                                              | status       | detail / note |
| ---------------------------------------------------- | ------------ | ------------- |
| strings.translates_string_method_call                | 🟢 supported |               |
| strings.translates_to_string_to_display              | 🟢 supported |               |
| strings.translates_string_concatenation_to_format    | 🟢 supported |               |
| strings.translates_string_predicate_methods          | 🟢 supported |               |
| strings.translates_string_repeat_and_replace         | 🟢 supported |               |
| strings.translates_string_replace_all_to_replace     | 🟢 supported |               |
| strings.translates_string_compound_append            | 🟢 supported |               |
| strings.translates_string_split_to_vec_string        | 🟢 supported |               |
| strings.translates_string_index_of_to_find           | 🟢 supported |               |
| strings.translates_string_slice_to_byte_range        | 🟢 supported |               |
| strings.translates_trim_start_end_to_trim_methods    | 🟢 supported |               |
| strings.translates_string_char_at_to_chars_nth       | 🟢 supported |               |
| strings.translates_string_pad_start_to_right_align   | 🟢 supported |               |
| strings.translates_string_pad_end_to_left_align      | 🟢 supported |               |
| strings.translates_string_pad_start_with_fill_char   | 🟢 supported |               |
| strings.translates_string_pad_end_with_fill_char     | 🟢 supported |               |
| strings.translates_string_char_code_at_to_code_point | 🟢 supported |               |
| strings.translates_string_from_char_code_to_char     | 🟢 supported |               |
| strings.translates_string_code_point_at              | 🟢 supported |               |
| strings.translates_string_concat_to_format           | 🟢 supported |               |
| strings.translates_string_at_to_chars_nth            | 🟢 supported |               |
| strings.translates_string_last_index_of_to_rfind     | 🟢 supported |               |
| strings.translates_string_lower_trim_methods         | 🟢 supported |               |
| strings.translates_string_ends_with_to_ends_with     | 🟢 supported |               |
| strings.translates_string_replace_substring_methods  | 🟢 supported |               |
| strings.translates_string_from_code_point            | 🟢 supported |               |
| strings.translates_string_value_of                   | 🟢 supported |               |
| strings.translates_string_is_well_formed             | 🟢 supported |               |
| strings.translates_string_to_well_formed             | 🟢 supported |               |

## types

| feature                                                          | status       | detail / note |
| ---------------------------------------------------------------- | ------------ | ------------- |
| types.translates_a_typed_function_returning_a_string             | 🟢 supported |               |
| types.translates_optional_field_to_option_and_fills_none         | 🟢 supported |               |
| types.translates_optional_field_supplied_wraps_some              | 🟢 supported |               |
| types.translates_generic_function_params                         | 🟢 supported |               |
| types.translates_default_param_to_option_unwrap_or_and_call_none | 🟢 supported |               |
| types.translates_default_param_supplied_wraps_some               | 🟢 supported |               |
| types.translates_locals_object_literal_and_field_access          | 🟢 supported |               |
| types.translates_nullable_to_option                              | 🟢 supported |               |
| types.translates_nullable_return_type                            | 🟢 supported |               |
| types.translates_enum_variant_construction                       | 🟢 supported |               |
| types.translates_object_keys_to_hashmap_keys                     | 🟢 supported |               |
| types.translates_object_values_to_hashmap_values                 | 🟢 supported |               |
| types.translates_discriminated_union_variant_construction        | 🟢 supported |               |
| types.translates_return_object_literal_to_struct_init            | 🟢 supported |               |
| types.translates_object_literal_argument_to_struct_init          | 🟢 supported |               |
| types.translates_record_computed_key_to_hashmap_entry            | 🟢 supported |               |
| types.translates_record_to_hashmap_literal                       | 🟢 supported |               |
| types.translates_hashmap_index_to_get                            | 🟢 supported |               |
| types.translates_hashmap_index_assign_to_insert                  | 🟢 supported |               |
| types.translates_object_is_nan_equal                             | 🟢 supported |               |
| types.translates_object_has_own_to_contains_key                  | 🟢 supported |               |
| types.translates_object_get_own_property_names_to_keys           | 🟢 supported |               |
| types.translates_object_assign_to_extend                         | 🟢 supported |               |
| types.translates_object_from_entries_to_collect                  | 🟢 supported |               |
| types.translates_object_freeze_to_passthrough                    | 🟢 supported |               |
| types.translates_object_is_frozen_to_false                       | 🟢 supported |               |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
