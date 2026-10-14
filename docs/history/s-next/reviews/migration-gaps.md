# 原 91 项测试迁移合同独立审计

2026-10-12，`/root/next_review`。此文是只读审计结果，不是作者的最终映射或新候选验收。本轮定位全部 91 个原测试函数，检查其主要断言，并与 r1 的语言、输入、机器和自举账本逐组对照。冻结旧测试不修改。

**身份分类无遗漏：91 个唯一名称与 seed.tests 精确相等，无重复、无额外项；68 项保留规则、23 项改变规则。执行覆盖不能由名称清单或新样本总数推出。** 多个旧测试包含一组独立断言，新的一个“同类”样本不能自动关闭全部旧分支。下表“已有对应证据”表示找到了该核心职责的实际新证据，不声明旧内部接口/每一个输入字符串均原样保留。

证据缩写：L=`build/snext-q-language-r1`（126）；I=`build/snext-q-input-r1`（24）；M=`build/q-machine-e3-r1`（79）；F=`build/snext-p4-r1`；R=独立 P4/参考探针。均绑定 r1 E3（SHA `57b3bb82…47ac7d3d`）或与之字节相同的 E2。旧源位置列是审计导航，不作为新身份。

## 集中缺口清单

- G1 Blocking：多层/递归/多require、300级链与顺序无关、非法策略、已有输出、missing capture、bad IR、65537 summary edge。root 已认领补专项，未见新冻结账本前仍待验。
- G2 资源：旧深度矩阵、2^32数组长度、巨大布局、3000级字段/索引/else/常量链。只要求原有拒绝/不崩溃合同，错误位置可按新明确语法更新。
- G3 诊断位置：原 exact_ast_and_recursive_header 测试跨第二个文件、类型/权限/递归头位置；新 initializer_span 不是全矩阵替代。
- G4 纯规则/常量：非法kind、unit比较、整数极值、add_size溢出、align 0/3/上溢，与统一常量入口对应；无需恢复两个旧 walker。
- G5 copy app：多个非空文件拼接与全空输出。当前 input probe 只 save(sources[1].bytes)，不能覆盖原应用行为。
- G6 ELF：原跨页text填充与精确头/零padding矩阵。
- G7 远边：实际执行 branch/call/address 后保留 x5/x6/x7/x28/x29/x30。S0 全指令字节 oracle 已很好覆盖编码，但测试入口跳过该指令区。
- G8 ABI：现有10参数聚合回传已通过；原合同额外测 unit 不占参数位置、u8/u32 进入callee之前的原始位、16字节栈对齐，以及无效 extern 聚合/切片/unit 参数。禁止用callee重新截位后的正确结果代替caller ABI断言。
- G9 名字身份：普通函数命名 addr_of 后仍调用该函数，不误认 builtin；极小正例即可。
- G10 清单：原16KiB/32源/8汇编边界、96字节路径/Unicode路径与空白/空分量、所有必选项缺失和重复的组合。I已覆盖部分，按原断言补缺而不是增加新路径规则。
- G11 输出：语法/名字/类型/入口/链接失败时保留已经存在的旧输出，并保持正确file身份；当前L主要验证不存在新输出。已有O_EXCL正常失败证据不能独自覆盖前序编译错误。
- G12 I/O：已有短IO/EINTR和partial write删除；原read/write/close故障的关闭FD证据尚需在新应用/owner runner中具体映射。
- G13 文本容量：Q现为8MiB，旧Rust的64MiB文本限制应标为范围变化并对当前阈值测试，不能据旧测试通过宣称当前门禁覆盖。

A1/A2 是旧Rust源码→GNU汇编/文本renderer专属接口，Q不维护这些出口；原意义必须映射为具体直接后端/S0/机器oracle责任，并明确已删除的内部接口断言。A3 是旧 --root CLI；Q只采用进程cwd。这里要求诚实分类，不建议为复活旧测试新增一个文本后端或CLI选项。

以下为完整91身份对照；详细机器可读版本为 `build/snext-reference-independent-r1/migration-audit.json`。本轮结束后按最终作者映射及新候选账本集中复验，不把增补实现中的文件当作通过。

| # | 原测试身份 | 审计状态 | 对应证据或缺口 |
| --- | --- | --- | --- |
| 1 | `aggregate_parameters_are_independent_and_results_use_hidden_slots` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 2 | `all_generations_resolve_modules_externs_and_reject_without_output` | 已有对应证据 | F+L：E1/E2/E3相同字节，L模块/extern/拒绝；E0独立P3账本 |
| 3 | `arbitrary_u64_constants_execute_and_write_exact_bytes` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 4 | `blocking_contracts_propagate_without_changing_generated_elf` | 待补/部分覆盖 | G1：正常三态已有；原多层/递归/多require矩阵须由新 Blocking 账本映射 |
| 5 | `blocking_long_chains_are_order_independent_and_do_not_skip_core_errors` | 待补/部分覆盖 | G1：300 级链、顺序无关、核心错误优先 |
| 6 | `blocking_missing_capture_invalid_ir_and_summary_capacity_fail_closed` | 待补/部分覆盖 | G1：missing capture、bad IR、65537 summary edge |
| 7 | `blocking_policy_is_strict_and_failure_preserves_existing_files` | 待补/部分覆盖 | G1：非法策略矩阵、已有文件保护 |
| 8 | `branches_loops_and_recursive_calls_preserve_frames` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 9 | `checked_arithmetic_and_shifts_really_stop` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 10 | `cleanup_reads_exit_values_but_cannot_capture_later_bindings` | 已有对应证据 | L defer_*snapshot、reject_defer_later_local；注册时快照替代退出时读取 |
| 11 | `cli_builds_the_target_language_manifest_scanner_and_copy_app` | 待补/部分覆盖 | G5：新 copy app 的实际清单/词法/复制组合 |
| 12 | `compiler_reaches_full_file_fixed_point_and_compiles_changed_behavior` | 已有对应证据 | F：E1=E2=E3、源码扰动后统计前缀改变 |
| 13 | `constant_walkers_agree_on_values_and_reject_boundaries` | 待补/部分覆盖 | G4：统一常量入口已实现；纯规则/常量运算非法矩阵应与原分支逐项对应 |
| 14 | `data_abs64_bss_and_segment_permissions_execute_against_reference` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 15 | `data_and_external_symbol_validation_rejects_malformed_images` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 16 | `data_strings_bss_and_root_relocations` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 17 | `defer_restrictions_and_raw_permissions_are_enforced` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 18 | `deferred_slice_checks_and_pointer_operations_remap` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 19 | `direct_and_external_toolchain_execute_the_same_target_program` | 职责替代须注明 | A1：Q 无生成 GNU 汇编出口；保留外部机器 oracle 和执行合同，不能声称原 copy 程序双出口比对仍存在 |
| 20 | `direct_build_needs_no_external_tools_and_is_deterministic` | 已有对应证据 | F：空 PATH、完整固定点和 guest trace |
| 21 | `elf_headers_segments_entry_and_padding_are_exact` | 待补/部分覆盖 | G6：text 页界 0/1021/1022/1023/1024 padding、完整头/零填充断言 |
| 22 | `explicit_bounds_pointer_offset_and_raw_storage_execute` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 23 | `explicit_returns_and_dead_code_are_checked` | 已有对应证据 | L：reject_dead_cleanup/panic路径；核心“死代码仍检查”不应仅引用无返回拒绝 |
| 24 | `external_c_calls_symbol_addresses_bitcasts_and_dynamic_bounds_execute` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 25 | `externs_are_scalar_unsafe_placeholders` | 待补/部分覆盖 | G8：已有unsafe extern正常路径；聚合结果/切片参数/unit参数拒绝矩阵需要明确对应 |
| 26 | `failures_preserve_stage_and_source_identity` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 27 | `far_control_flow_calls_addresses_and_scratch_contract_execute` | 待补/部分覆盖 | G7：实际执行远跳/call/address 后的 scratch 保持；已有 S0 oracle 主要检验编码 |
| 28 | `fixed_grammar_encodes_all_operations_and_localizes_labels` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 29 | `frontend_accepts_forward_layout_snapshots_results_and_cleanup` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 30 | `frontend_checks_actual_bootstrap_sources_and_ir_block_contract` | 已有对应证据 | F：Q真实源码三代；M typed IR block验证 |
| 31 | `frontend_depth_limits_and_type_shadowing_fail_before_guest_stack_overflow` | 待补/部分覆盖 | G2：表达式/block/指针/类型链深度；类型名字空间改变需说明 |
| 32 | `frontend_merges_module_imports_and_accepts_prefix_namespaces` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 33 | `frontend_rejects_invalid_programs_with_source_diagnostics` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 34 | `frontend_reports_exact_ast_and_recursive_header_locations` | 待补/部分覆盖 | G3：跨第2文件、递归头、具体错误 token 位置矩阵；现有 initializer_span 只覆盖一项 |
| 35 | `frontend_reports_permission_and_capacity_errors_without_target_panics` | 待补/部分覆盖 | G2：2^32 长度、巨大布局和长链容量诊断 |
| 36 | `frontend_separates_cursor_diagnostics_and_reuses_function_arenas` | 已有对应证据 | L function_reuse；M AST世代；R/P4 owner复用 |
| 37 | `frontend_static_layout_keeps_large_zero_arrays_in_bss` | 已有对应证据 | F+M：Q自举源的大静态arena/小ELF和ABS64/BSS；原布局尺寸变化应说明 |
| 38 | `hidden_result_unit_omission_and_over_eight_positions_follow_lp64` | 待补/部分覆盖 | G8：已有10参数/聚合回传；还需 unit 不占位置、窄整数 raw ABI 与栈16对齐 |
| 39 | `instructions_match_independent_clang_lld_bytes` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 40 | `integer_widths_wrapping_comparisons_and_lp64_results_execute` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 41 | `io_failures_close_files_and_remove_only_the_new_output` | 待补/部分覆盖 | G12：partial write 清除与已有输出保留已测；原 read/write/close失败 FD close trace 还需映射 |
| 42 | `literals_do_not_unify_across_comparison_operands` | 已有对应证据 | L compare_direction_reject/const_comparison/reference_literal_bad；规则已变更 |
| 43 | `malformed_and_unsupported_inputs_have_file_and_line_spans` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 44 | `malformed_externs_slices_and_new_ir_are_rejected` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 45 | `malformed_ir_returns_diagnostics_without_source_rechecking` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 46 | `malformed_manifest_missing_source_scanner_error_and_limits_fail_cleanly` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 47 | `malformed_operands_symbols_entries_and_ranges_are_rejected` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 48 | `merged_imports_prefix_modules_and_shadowing` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 49 | `modules_merge_and_private_boundaries` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 50 | `normal_function_named_like_builtin_keeps_its_identity` | 待补/部分覆盖 | G9：addr_of 同名普通函数应直接调用，尚无命名专项 |
| 51 | `output_failures_and_source_aliases_are_safe` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 52 | `overlap_copy_large_stack_offsets_and_zero_size_values_execute` | 已有对应证据 | M machine-overlap-copy、ir-frame_limit；R zero_size |
| 53 | `project::tests::bounds_manifest_and_input_counts` | 待补/部分覆盖 | G10：16KiB及32/33 source、8/9 assembly 边界 |
| 54 | `project::tests::preserves_input_order_and_unicode_paths` | 待补/部分覆盖 | G10：Unicode路径及实际多文件顺序；现 input 只复制首文件 |
| 55 | `project::tests::rejects_ambiguous_and_escaping_paths` | 待补/部分覆盖 | G10：96/97字节、空分量、Unicode空白等旧路径矩阵只部分有覆盖 |
| 56 | `project::tests::rejects_duplicates_missing_fields_and_unknowns` | 待补/部分覆盖 | G10：已有重复entry/unknown，缺字段/重复其它项矩阵需对应 |
| 57 | `project_builds_without_tools_is_deterministic_and_resolves_paths_from_root` | 职责替代须注明 | A3：无外部工具/确定性由 F 覆盖；Q cwd 路径不提供旧CLI --root，映射须注明接口删除 |
| 58 | `project_cannot_publish_over_any_input_alias` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 59 | `project_errors_keep_outputs_and_report_the_correct_source` | 待补/部分覆盖 | G11：已有安全发布机制；需明确不同编译失败保留旧输出、正确file身份的对应样本 |
| 60 | `pure_rules_use_only_explicit_scalar_and_layout_inputs` | 待补/部分覆盖 | G4：纯规则 invalid kind/unit/add_size/align 边界未由合法机器算术替代 |
| 61 | `real_program_reads_scans_and_copies_multiple_files_and_empty_input` | 待补/部分覆盖 | G5：三文件拼接、全空输出；当前 input probe 仅保存第1文件 |
| 62 | `real_source_direct_and_reference_paths_agree` | 职责替代须注明 | A1：源码同程序 direct/文本外部后端已收缩；各语义移入 L，外部机器一致性移入 M，须记录替代范围 |
| 63 | `real_source_resolves_forward_constants_layouts_and_calls` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 64 | `rejected_inputs_never_replace_existing_output` | 待补/部分覆盖 | G11：当前语言拒绝在空输出目录；已有输出下语法/类型/entry/链接拒绝需对应 |
| 65 | `return_snapshot_precedes_defer_and_captured_binding_is_stable` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 66 | `scanner_reports_token_ranges_and_error_offsets` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 67 | `self_compiler_diagnostic_change_is_translated_and_check_creates_no_output` | 已有对应证据 | F：行为扰动由诊断字样改为统计前缀，check无输出；应写明扰动观测对象变化 |
| 68 | `short_io_and_eintr_are_retried_in_target_code` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 69 | `slices_pointer_builtins_and_permissions` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 70 | `source_capacity_is_a_resource_diagnostic` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 71 | `source_result_construction_propagation_and_match_compose` | 已有对应证据 | L enum_* + reject_question；旧 Result/? 明确删除，显式match替代 |
| 72 | `source_snapshots_cleanup_and_raw_access_reach_the_checker` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 73 | `static_validation_visits_invalid_tails_and_empty_repeats` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 74 | `statics_validate_arrays_records_constants_and_permissions` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
| 75 | `target_constants_match_rust_and_independent_assembler` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 76 | `target_input_keeps_manifest_snapshots_and_handles_short_io` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 77 | `target_ir_aggregate_abi_snapshot_abs64_bss_and_checked_panic` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 78 | `target_lexer_decodes_numbers_strings_ranges_and_nested_comments` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 79 | `target_manifest_limits_and_source_aliases_preserve_inputs` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 80 | `target_preserves_semantic_ir_and_reuses_sparse_label_ranges` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 81 | `target_rejects_bad_numeric_tokens_utf8_and_unsupported_identifiers` | 已有对应证据 | I：输入/lexer/错误/短IO对应场景；不表示原应用实现保留 |
| 82 | `target_rejects_invalid_ir_and_link_data_before_output` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 83 | `target_requires_label_one_and_finite_value_layouts` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 84 | `target_s0_all_operations_far_fixups_match_independent_bytes` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 85 | `target_s0_rejects_syntax_ranges_and_unsupported_grammar` | 已有对应证据 | M：机器/IR/布局/符号/S0 对应场景；以新具体case为准 |
| 86 | `text_capacity_is_checked_before_output_allocation` | 待补/部分覆盖 | G13：Q 8MiB文本限额；旧Rust64MiB限额不原样适用 |
| 87 | `typed_address_rendering_and_generated_labels_execute_without_collisions` | 职责替代须注明 | A2：Rust renderer 名字生成不属 Q；改为 S0 两文件同名 .L 标签/符号地址的等价职责 |
| 88 | `typed_control_flow_preserves_live_temporaries_across_far_edges` | 职责替代须注明 | A2 + G7：Rust renderer 移除；远边 scratch 保持仍需实际执行 |
| 89 | `typed_lowering_exposes_checks_and_renderer_rejects_malformed_operations` | 职责替代须注明 | A2：内部 Rust enum/text renderer 移除；用 typed IR 验证和 S0非法操作替代，不保留旧内部IR形状断言 |
| 90 | `unary_casts_unit_comparison_and_unreachable_execute` | 已有对应证据 | M operators + R cast/panic；unit 比较和不可达 lowering 应在 G4/G8 补例中明确 |
| 91 | `unsupported_features_are_distinct_from_language_errors` | 已有对应证据 | L：语言/模块/权限/常量/初始化/cleanup 对应场景；按迁移后的规则判定 |
