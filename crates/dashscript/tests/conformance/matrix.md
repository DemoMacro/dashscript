# DashScript Conformance Matrix

- 398 features: **331** supported, **1** partial, **0** unsupported, **66** untested
- correctness cases passing: 2

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
| array.at                                                     | 🟢 supported |               |
| array.concat                                                 | 🟢 supported |               |
| array.copyWithin                                             | ⚪ untested  |               |
| array.entries                                                | 🟢 supported |               |
| array.every                                                  | 🟢 supported |               |
| array.fill                                                   | 🟢 supported |               |
| array.filter                                                 | 🟢 supported |               |
| array.find                                                   | 🟢 supported |               |
| array.findIndex                                              | 🟢 supported |               |
| array.findLast                                               | 🟢 supported |               |
| array.findLastIndex                                          | 🟢 supported |               |
| array.flat                                                   | 🟢 supported |               |
| array.flatMap                                                | 🟢 supported |               |
| array.forEach                                                | 🟢 supported |               |
| array.from                                                   | 🟢 supported |               |
| array.fromAsync                                              | ⚪ untested  |               |
| array.includes                                               | 🟢 supported |               |
| array.indexOf                                                | 🟢 supported |               |
| array.isArray                                                | 🟢 supported |               |
| array.join                                                   | 🟢 supported |               |
| array.keys                                                   | 🟢 supported |               |
| array.lastIndexOf                                            | 🟢 supported |               |
| array.map                                                    | 🟢 supported |               |
| array.of                                                     | 🟢 supported |               |
| array.pop                                                    | 🟢 supported |               |
| array.push                                                   | 🟢 supported |               |
| array.reduce                                                 | 🟢 supported |               |
| array.reduceRight                                            | 🟢 supported |               |
| array.reverse                                                | 🟢 supported |               |
| array.shift                                                  | 🟢 supported |               |
| array.slice                                                  | 🟢 supported |               |
| array.some                                                   | 🟢 supported |               |
| array.sort                                                   | 🟢 supported |               |
| array.splice                                                 | 🟢 supported |               |
| array.toLocaleString                                         | ⚪ untested  |               |
| array.toReversed                                             | 🟢 supported |               |
| array.toSorted                                               | 🟢 supported |               |
| array.toSpliced                                              | 🟢 supported |               |
| array.toString                                               | ⚪ untested  |               |
| array.unshift                                                | 🟢 supported |               |
| array.values                                                 | 🟢 supported |               |
| array.with                                                   | 🟢 supported |               |

## clone_move

| feature                                                         | status       | detail / note |
| --------------------------------------------------------------- | ------------ | ------------- |
| clone_move.translates_unmutated_let_is_plain_let                | 🟢 supported |               |
| clone_move.translates_mutated_let_is_let_mut                    | 🟢 supported |               |
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

## correctness

| feature                     | status       | detail / note                                                                  |
| --------------------------- | ------------ | ------------------------------------------------------------------------------ |
| correctness.parse_int_radix | 🟢 supported | parseInt('ff', 16) prints 255 _correct: true_                                  |
| correctness.array_join      | 🟢 supported | [1,2,3].join('-') prints 1-2-3 (f64 Display drops trailing .0) _correct: true_ |

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
| number.EPSILON                                               | 🟢 supported |               |
| number.MAX_SAFE_INTEGER                                      | ⚪ untested  |               |
| number.MAX_VALUE                                             | 🟢 supported |               |
| number.MIN_SAFE_INTEGER                                      | ⚪ untested  |               |
| number.MIN_VALUE                                             | ⚪ untested  |               |
| number.NEGATIVE_INFINITY                                     | ⚪ untested  |               |
| number.NaN                                                   | 🟢 supported |               |
| number.POSITIVE_INFINITY                                     | 🟢 supported |               |
| number.isFinite                                              | 🟢 supported |               |
| number.isInteger                                             | 🟢 supported |               |
| number.isNaN                                                 | 🟢 supported |               |
| number.isSafeInteger                                         | 🟢 supported |               |
| number.parseFloat                                            | 🟢 supported |               |
| number.parseInt                                              | 🟢 supported |               |
| number.toExponential                                         | 🟢 supported |               |
| number.toFixed                                               | 🟢 supported |               |
| number.toLocaleString                                        | ⚪ untested  |               |
| number.toPrecision                                           | ⚪ untested  |               |
| number.toString                                              | 🟢 supported |               |
| number.valueOf                                               | 🟢 supported |               |
| object.assign                                                | 🟢 supported |               |
| object.constructor                                           | ⚪ untested  |               |
| object.create                                                | ⚪ untested  |               |
| object.defineGetter                                          | ⚪ untested  |               |
| object.defineProperties                                      | ⚪ untested  |               |
| object.defineProperty                                        | ⚪ untested  |               |
| object.defineSetter                                          | ⚪ untested  |               |
| object.entries                                               | 🟢 supported |               |
| object.freeze                                                | ⚪ untested  |               |
| object.fromEntries                                           | 🟢 supported |               |
| object.getOwnPropertyDescriptor                              | ⚪ untested  |               |
| object.getOwnPropertyDescriptors                             | ⚪ untested  |               |
| object.getOwnPropertyNames                                   | 🟢 supported |               |
| object.getOwnPropertySymbols                                 | ⚪ untested  |               |
| object.getPrototypeOf                                        | ⚪ untested  |               |
| object.groupBy                                               | ⚪ untested  |               |
| object.hasOwn                                                | 🟢 supported |               |
| object.hasOwnProperty                                        | ⚪ untested  |               |
| object.is                                                    | 🟢 supported |               |
| object.isExtensible                                          | ⚪ untested  |               |
| object.isFrozen                                              | ⚪ untested  |               |
| object.isPrototypeOf                                         | ⚪ untested  |               |
| object.isSealed                                              | ⚪ untested  |               |
| object.keys                                                  | 🟢 supported |               |
| object.lookupGetter                                          | ⚪ untested  |               |
| object.lookupSetter                                          | ⚪ untested  |               |
| object.preventExtensions                                     | ⚪ untested  |               |
| object.propertyIsEnumerable                                  | ⚪ untested  |               |
| object.proto                                                 | ⚪ untested  |               |
| object.seal                                                  | ⚪ untested  |               |
| object.setPrototypeOf                                        | ⚪ untested  |               |
| object.toLocaleString                                        | ⚪ untested  |               |
| object.toString                                              | ⚪ untested  |               |
| object.valueOf                                               | ⚪ untested  |               |
| object.values                                                | 🟢 supported |               |
| global.parseInt                                              | 🟢 supported |               |
| global.parseFloat                                            | 🟢 supported |               |
| global.isNaN                                                 | ⚪ untested  |               |
| global.isFinite                                              | ⚪ untested  |               |
| global.encodeURI                                             | ⚪ untested  |               |
| global.decodeURI                                             | ⚪ untested  |               |
| global.Number                                                | 🟢 supported |               |
| global.String                                                | 🟢 supported |               |
| global.Boolean                                               | 🟢 supported |               |

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
| math.E                                          | 🟢 supported |               |
| math.LN10                                       | 🟢 supported |               |
| math.LN2                                        | 🟢 supported |               |
| math.LOG10E                                     | 🟢 supported |               |
| math.LOG2E                                      | 🟢 supported |               |
| math.PI                                         | 🟢 supported |               |
| math.SQRT1_2                                    | 🟢 supported |               |
| math.SQRT2                                      | 🟢 supported |               |
| math.abs                                        | 🟢 supported |               |
| math.acos                                       | 🟢 supported |               |
| math.acosh                                      | 🟢 supported |               |
| math.asin                                       | 🟢 supported |               |
| math.asinh                                      | 🟢 supported |               |
| math.atan                                       | 🟢 supported |               |
| math.atan2                                      | 🟢 supported |               |
| math.atanh                                      | 🟢 supported |               |
| math.cbrt                                       | 🟢 supported |               |
| math.ceil                                       | 🟢 supported |               |
| math.clz32                                      | 🟢 supported |               |
| math.cos                                        | 🟢 supported |               |
| math.cosh                                       | 🟢 supported |               |
| math.exp                                        | 🟢 supported |               |
| math.expm1                                      | 🟢 supported |               |
| math.f16round                                   | ⚪ untested  |               |
| math.floor                                      | 🟢 supported |               |
| math.fround                                     | 🟢 supported |               |
| math.hypot                                      | 🟢 supported |               |
| math.imul                                       | 🟢 supported |               |
| math.log                                        | 🟢 supported |               |
| math.log10                                      | 🟢 supported |               |
| math.log1p                                      | 🟢 supported |               |
| math.log2                                       | 🟢 supported |               |
| math.max                                        | 🟢 supported |               |
| math.min                                        | 🟢 supported |               |
| math.pow                                        | 🟢 supported |               |
| math.random                                     | ⚪ untested  |               |
| math.round                                      | 🟢 supported |               |
| math.sign                                       | 🟢 supported |               |
| math.sin                                        | 🟢 supported |               |
| math.sinh                                       | 🟢 supported |               |
| math.sqrt                                       | 🟢 supported |               |
| math.sumPrecise                                 | ⚪ untested  |               |
| math.tan                                        | 🟢 supported |               |
| math.tanh                                       | 🟢 supported |               |
| math.trunc                                      | 🟢 supported |               |

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

| feature                                             | status       | detail / note |
| --------------------------------------------------- | ------------ | ------------- |
| operators.translates_arithmetic_and_comparison      | 🟢 supported |               |
| operators.translates_logical_and_unary              | 🟢 supported |               |
| operators.translates_compound_assignment            | 🟢 supported |               |
| operators.translates_template_literal               | 🟢 supported |               |
| operators.translates_ternary_to_if_expression       | 🟢 supported |               |
| operators.translates_length_to_len                  | 🟢 supported |               |
| operators.translates_exponent_operator              | 🟢 supported |               |
| operators.translates_in_operator_to_contains_key    | 🟢 supported |               |
| operators.translates_arrow_function_expression_body | 🟢 supported |               |
| operators.translates_field_assign_to_field          | 🟢 supported |               |
| operators.translates_bitwise_and_or_xor             | 🟢 supported |               |
| operators.translates_bitwise_shifts                 | 🟢 supported |               |
| operators.translates_bitwise_not                    | 🟢 supported |               |
| operators.translates_bitwise_compound_assign        | 🟢 supported |               |

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
| strings.translates_string_to_locale_lower            | 🟢 supported |               |
| strings.translates_string_to_locale_upper            | 🟢 supported |               |
| strings.translates_string_is_well_formed             | 🟢 supported |               |
| strings.translates_string_to_well_formed             | 🟢 supported |               |
| string.anchor                                        | ⚪ untested  |               |
| string.at                                            | 🟢 supported |               |
| string.big                                           | ⚪ untested  |               |
| string.blink                                         | ⚪ untested  |               |
| string.bold                                          | ⚪ untested  |               |
| string.charAt                                        | 🟢 supported |               |
| string.charCodeAt                                    | 🟢 supported |               |
| string.codePointAt                                   | 🟢 supported |               |
| string.concat                                        | 🟢 supported |               |
| string.endsWith                                      | 🟢 supported |               |
| string.fixed                                         | ⚪ untested  |               |
| string.fontcolor                                     | ⚪ untested  |               |
| string.fontsize                                      | ⚪ untested  |               |
| string.fromCharCode                                  | 🟢 supported |               |
| string.fromCodePoint                                 | 🟢 supported |               |
| string.includes                                      | 🟢 supported |               |
| string.indexOf                                       | 🟢 supported |               |
| string.isWellFormed                                  | 🟢 supported |               |
| string.italics                                       | ⚪ untested  |               |
| string.lastIndexOf                                   | 🟢 supported |               |
| string.link                                          | ⚪ untested  |               |
| string.localeCompare                                 | ⚪ untested  |               |
| string.match                                         | ⚪ untested  |               |
| string.matchAll                                      | ⚪ untested  |               |
| string.normalize                                     | ⚪ untested  |               |
| string.padEnd                                        | 🟢 supported |               |
| string.padStart                                      | 🟢 supported |               |
| string.raw                                           | ⚪ untested  |               |
| string.repeat                                        | 🟢 supported |               |
| string.replace                                       | 🟢 supported |               |
| string.replaceAll                                    | 🟢 supported |               |
| string.search                                        | ⚪ untested  |               |
| string.slice                                         | 🟢 supported |               |
| string.small                                         | ⚪ untested  |               |
| string.split                                         | 🟢 supported |               |
| string.startsWith                                    | 🟢 supported |               |
| string.strike                                        | ⚪ untested  |               |
| string.sub                                           | ⚪ untested  |               |
| string.substr                                        | ⚪ untested  |               |
| string.substring                                     | 🟢 supported |               |
| string.sup                                           | ⚪ untested  |               |
| string.toLocaleLowerCase                             | 🟢 supported |               |
| string.toLocaleUpperCase                             | 🟢 supported |               |
| string.toLowerCase                                   | 🟢 supported |               |
| string.toString                                      | ⚪ untested  |               |
| string.toUpperCase                                   | 🟢 supported |               |
| string.toWellFormed                                  | 🟢 supported |               |
| string.trim                                          | 🟢 supported |               |
| string.trimEnd                                       | 🟢 supported |               |
| string.trimStart                                     | 🟢 supported |               |
| string.unicode_code_point_escapes                    | ⚪ untested  |               |
| string.valueOf                                       | 🟢 supported |               |

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

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
