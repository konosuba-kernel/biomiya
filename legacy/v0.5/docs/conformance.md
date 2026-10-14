# biomiya 0.5 合规范基线

本文件与 [reference](reference.md)、[RV64 目标补充](architecture-rv64.md) 一起规定规范验收基线。用例中的预期不随实现结果改变；实际结果由 `tools/conformance.py` 输出。它们不是覆盖全部语言的完备测试集，完整交付另需 [workflow](workflow.md) 与 [条款追踪](core-coverage.json) 的审查和专项验收。

## 判定与运行协议

机器可读索引为 [manifest.json](../tests/conformance/manifest.json)。每个 `.vt` 是独立编译单元，不与其他用例合并，目标固定为 rv64-core-v1。除 manifest 列出的扩展外，全部 Extend 关闭。

| kind | 必须满足的结果 |
| --- | --- |
| accept | 完成 Core 前端检查；不要求链接缺省运行平台 |
| reject | 前端拒绝并匹配 category 类别；不能因“未实现”而算通过 |
| run | 编译、链接并调用 `pub fn test() -> i32`，正常返回 result |
| panic | 同样调用 test，必须进入 panic 钩子且不正常返回；不比较消息文本 |
| extend-reject | Core 接受；启用所列扩展后拒绝并匹配验证类别 |
| extend-accept | Core 与所列扩展都接受，防止验证器以全部拒绝代替正确实现 |

测试适配器负责按普通 biomiya ABI 调用 test，并提供启动及 panic 观察钩子；它不属于源语言。运行在仿真器或硬件上均可，但不得把宿主解释器结果冒充目标代码验证。运行超时是测试基础设施失败，不能作为 panic 或验证失败通过。accept／extend 用例包含不应执行的程序；不能随意运行它们以猜测静态判定。

诊断类别见 reference 附录 G。先做 Core 检查再做 Extend；同时有多种错误的输入不要求固定诊断顺序，但这里的负例按其单一主要问题验收。

## 当前用例

| ID | kind | 条款 | 预期 |
| --- | --- | --- | --- |
| [run_integer_context](../tests/conformance/run_integer_context.vt) | run | 6.12;12.2 | 4 |
| [run_generic_forward](../tests/conformance/run_generic_forward.vt) | run | 4.1;10.2 | 37 |
| [run_nested_match](../tests/conformance/run_nested_match.vt) | run | 13.3 | 2 |
| [run_record_order](../tests/conformance/run_record_order.vt) | run | 5.2;28 | 21 |
| [run_assignment_order](../tests/conformance/run_assignment_order.vt) | run | 8.3 | 19 |
| [run_compound_order](../tests/conformance/run_compound_order.vt) | run | 12.6 | 21 |
| [run_defer_return](../tests/conformance/run_defer_return.vt) | run | 15.3 | 723 |
| [run_question_defer](../tests/conformance/run_question_defer.vt) | run | 11.4;15.3 | 58 |
| [run_loop_cleanup](../tests/conformance/run_loop_cleanup.vt) | run | 14.1;15.2 | 2 |
| [run_closed_max](../tests/conformance/run_closed_max.vt) | run | 12.5;14.1 | 2 |
| [run_method_projection](../tests/conformance/run_method_projection.vt) | run | 11.9 | 7 |
| [run_tuple_array_equality](../tests/conformance/run_tuple_array_equality.vt) | run | 11.8;12.3 | 1 |
| [run_slice_split](../tests/conformance/run_slice_split.vt) | run | 9.3;14.1 | 56 |
| [run_atomic](../tests/conformance/run_atomic.vt) | run | 21.3;22 | 262 |
| [run_const_branch](../tests/conformance/run_const_branch.vt) | run | 26 | 8 |
| [run_aggregate_call](../tests/conformance/run_aggregate_call.vt) | run | RV64:R3 | 92 |
| [run_rv64_layout](../tests/conformance/run_rv64_layout.vt) | run | RV64:R2;RV64:R6 | 0 |
| [run_asm](../tests/conformance/run_asm.vt) | run | RV64:R8 | 12 |
| [panic_overflow](../tests/conformance/panic_overflow.vt) | panic | 12.2 | panic |
| [panic_bounds](../tests/conformance/panic_bounds.vt) | panic | 11.8 | panic |
| [panic_shift](../tests/conformance/panic_shift.vt) | panic | 12.2 | panic |
| [accept_unused_generic](../tests/conformance/accept_unused_generic.vt) | accept | 10.2 | 通过 |
| [accept_move_without_analysis](../tests/conformance/accept_move_without_analysis.vt) | accept | 7.1;8.5 | 通过 |
| [accept_special_transfer](../tests/conformance/accept_special_transfer.vt) | accept | 27;RV64:R6 | 通过 |
| [reject_incomplete_match](../tests/conformance/reject_incomplete_match.vt) | reject | 13.3 | E_PATTERN |
| [reject_refutable_let](../tests/conformance/reject_refutable_let.vt) | reject | 13.3 | E_PATTERN |
| [reject_implicit_array_slice](../tests/conformance/reject_implicit_array_slice.vt) | reject | 6.12 | E_TYPE |
| [reject_late_inference](../tests/conformance/reject_late_inference.vt) | reject | 6.12 | E_TYPE |
| [reject_immutable_write](../tests/conformance/reject_immutable_write.vt) | reject | 8.2 | E_TYPE |
| [reject_unknown_attribute](../tests/conformance/reject_unknown_attribute.vt) | reject | 5.8 | E_ATTRIBUTE |
| [reject_bad_atomic_order](../tests/conformance/reject_bad_atomic_order.vt) | reject | 22 | E_CONST |
| [reject_const_dead_call](../tests/conformance/reject_const_dead_call.vt) | reject | 26 | E_CONST |
| [reject_dead_type_error](../tests/conformance/reject_dead_type_error.vt) | reject | 1.5;6.12 | E_TYPE |
| [reject_missing_result](../tests/conformance/reject_missing_result.vt) | reject | 5.4 | E_TYPE |
| [reject_special_call](../tests/conformance/reject_special_call.vt) | reject | 27 | E_CONTEXT |
| [reject_non_tail_transfer](../tests/conformance/reject_non_tail_transfer.vt) | reject | 27;RV64:R6 | E_CONTEXT |
| [reject_raw_without_unsafe](../tests/conformance/reject_raw_without_unsafe.vt) | reject | 17.1 | E_UNSAFE |
| [reject_duplicate_field](../tests/conformance/reject_duplicate_field.vt) | reject | 5.2 | E_TYPE |
| [reject_polymorphic_recursion](../tests/conformance/reject_polymorphic_recursion.vt) | reject | 10.3 | E_INSTANTIATION |
| [reject_recursive_layout](../tests/conformance/reject_recursive_layout.vt) | reject | 10.3 | E_LAYOUT |
| [reject_raw_autoderef](../tests/conformance/reject_raw_autoderef.vt) | reject | 11.9 | E_TYPE |
| [reject_one_tuple](../tests/conformance/reject_one_tuple.vt) | reject | 6.6;appendix A | E_PARSE |
| [reject_uninitialized_binding](../tests/conformance/reject_uninitialized_binding.vt) | reject | 8.1 | E_PARSE |
| [reject_asm_register](../tests/conformance/reject_asm_register.vt) | reject | RV64:R8 | E_TARGET |
| [extend_resource_discard](../tests/conformance/extend_resource_discard.vt) | extend-reject | 30.3 | V_OWNERSHIP |
| [extend_move_join](../tests/conformance/extend_move_join.vt) | extend-reject | 30.2 | V_OWNERSHIP |
| [extend_borrow_generic](../tests/conformance/extend_borrow_generic.vt) | extend-reject | 31.5 | V_BORROW |
| [extend_alias_conflict](../tests/conformance/extend_alias_conflict.vt) | extend-reject | 31.1 | V_BORROW |
| [extend_blocking_call](../tests/conformance/extend_blocking_call.vt) | extend-reject | 32.1 | V_BLOCKING |
| [extend_resource_consume](../tests/conformance/extend_resource_consume.vt) | extend-accept | 30.3 | 通过 |
| [extend_borrow_local](../tests/conformance/extend_borrow_local.vt) | extend-accept | 31.1;31.2 | 通过 |
| [extend_effect_widen](../tests/conformance/extend_effect_widen.vt) | extend-accept | 32.2 | 通过 |
| [run_call_snapshot](../tests/conformance/run_call_snapshot.vt) | run | 11.3;RV64:R3 | 29 |
| [run_nested_generic_layout](../tests/conformance/run_nested_generic_layout.vt) | run | 10.3;RV64:R2 | 0 |
| [run_repeat_array](../tests/conformance/run_repeat_array.vt) | run | 11.1;26 | 22 |
| [run_raw_address](../tests/conformance/run_raw_address.vt) | run | 18.4;24 | 10 |
| [run_uninit_fields](../tests/conformance/run_uninit_fields.vt) | run | 18.4;21.1 | 37 |
| [run_raw_slice](../tests/conformance/run_raw_slice.vt) | run | 9.3;18.4 | 16 |
| [run_const_raw_address](../tests/conformance/run_const_raw_address.vt) | run | 18.4;26;RV64:R1 | 7 |
| [accept_extern_static](../tests/conformance/accept_extern_static.vt) | accept | 5.7;18.4;25.3 | 通过 |
| [reject_nested_recursive_layout](../tests/conformance/reject_nested_recursive_layout.vt) | reject | 10.3 | E_LAYOUT |
| [reject_growing_wrapped_layout](../tests/conformance/reject_growing_wrapped_layout.vt) | reject | 10.3 | E_LAYOUT |
| [reject_raw_address_rvalue](../tests/conformance/reject_raw_address_rvalue.vt) | reject | 18.4 | E_TYPE |
| [reject_raw_address_immutable](../tests/conformance/reject_raw_address_immutable.vt) | reject | 18.4 | E_TYPE |
| [reject_raw_slice_without_unsafe](../tests/conformance/reject_raw_slice_without_unsafe.vt) | reject | 9.3;17.1 | E_UNSAFE |
| [reject_raw_slice_space](../tests/conformance/reject_raw_slice_space.vt) | reject | 9.3;20 | E_TYPE |
| [reject_readonly_slice_pointer](../tests/conformance/reject_readonly_slice_pointer.vt) | reject | 9.3 | E_TYPE |
| [reject_repeat_noncopy_zero](../tests/conformance/reject_repeat_noncopy_zero.vt) | reject | 11.1 | E_TYPE |
| [reject_repeat_dynamic](../tests/conformance/reject_repeat_dynamic.vt) | reject | 11.1;26 | E_CONST |
| [reject_repeat_const_form](../tests/conformance/reject_repeat_const_form.vt) | reject | 11.1;appendix A | E_PARSE |
| [reject_extern_static_type](../tests/conformance/reject_extern_static_type.vt) | reject | 5.7;25.1 | E_TYPE |
| [reject_extern_static_without_unsafe](../tests/conformance/reject_extern_static_without_unsafe.vt) | reject | 5.7;18.4 | E_UNSAFE |
| [run_uninit_array](../tests/conformance/run_uninit_array.vt) | run | 18.4;21.1 | 10 |
| [run_raw_address_shadow](../tests/conformance/run_raw_address_shadow.vt) | run | 4.5;18.4 | 11 |
| [panic_raw_address_bounds](../tests/conformance/panic_raw_address_bounds.vt) | panic | 18.4 | panic |
| [extend_raw_address_preserves_owner](../tests/conformance/extend_raw_address_preserves_owner.vt) | extend-accept | 18.4;30.1 | 通过 |

## 后端与平台补充验收

上面的源码用例之外，以下集成场景是声明完整目标支持前的必需项目；需要平台／外部工具链测试夹具，不能仅以前端接受替代。

| 场景 | 验收要求 |
| --- | --- |
| 模块 | 同一模块跨文件前向引用；重复定义拒绝；私有成员跨模块拒绝；输入文件顺序不改变结果 |
| C ABI | 与独立 C 对象双向调用：超过八个标量、1／2／超过 2 个 XLEN 大小的结构体、寄存器与栈交界、函数指针及间接结果 |
| 调用快照 | 先按值传聚合，再通过另一个实参修改源；callee 必须观察求值时的快照 |
| 原始存储 | 局部／静态／packed 字段取址不读取目标；MaybeUninit 逐字段初始化后才能 assume_init；切片空视图和指针成员保持地址／长度，不隐式移动源视图 |
| 外部数据与重定位 | 独立 C 对象提供标量及正长度数组，双向读写；链接脚本边界符号仅取址且无加载；本地静态字段常量地址重定位；缺失符号与可变性／函数对象种类冲突拒绝 |
| 原子 | 多 hart 发布／获取、强 CAS 的 LR/SC 内部重试、窄原子不修改邻接普通字段；单线程源码用例不证明完整内存模型 |
| MMIO | 用可观测设备模型检查访问次数、宽度与设备 fence；不得只读取 RAM 代替 |
| 陷阱 | 保存并恢复 x1—x31、sepc／sstatus；trap_entry defer 先于 handler；handler defer 先于返回；嵌套帧不覆盖；非法普通调用拒绝 |
| 链接与运行时 | 缺少 panic／dispatch 等实际使用符号时构建失败；section 权限、对象对齐、启动静态数据和 panic 不展开符合目标合同 |

这些项目不要求每个编译器使用同一种内部 IR 或测试框架。Core 前端可先通过源码检查，再逐步加入链接与目标运行，不能把阶段性完成表述为全部合规范。

## 原子并发判定场景

以下是第 22 章的多上下文验收约束，需使用多 hart 夹具或内存模型工具。初始值均在启动测试上下文前正确发布；每行只执行列出的写入。它们不是已经执行的测试。允许的弱顺序结果不要求后端一定出现，因为实现可以提供更强顺序；有限次运行未观察到禁止结果也不能单独证明模型正确。

| 场景 | 操作与观察 | 判定 |
| --- | --- | --- |
| SeqCst 存储缓冲 | x=y=0；A 先写 x=1 再读 y，B 先写 y=1 再读 x，四次操作均 SeqCst | 两次读取同时为 0 禁止 |
| Relaxed 存储缓冲 | 同上，四次操作均 Relaxed | 两次读取同时为 0 允许，不要求出现 |
| 读读一致性 | A 对 x 先写 1 再写 2；B 连续两次读 x，全部 Relaxed | B 先读 2 再读 1 禁止 |
| RMW 前驱 | x=0；两个上下文各执行一次 fetch_add(1, Relaxed) | 返回值必须为 0 和 1，不能均为 0 |
| 发布／获取 | A 写普通 data=7 后 Release 写 flag=1；B Acquire 读到这次 flag=1 后读 data | data 必须为 7 |
| Release sequence | 在上一发布后，第三上下文以 Relaxed RMW 将 flag 从 1 增为 2；B Acquire 读到这次 2 后读 data | data 必须为 7 |
| 强 CAS | 已同步初值 x=0，无其他写入；compare_exchange(0, 1, SeqCst, Acquire) | 必须 Ok(0)，不能因 LR/SC 保留集丢失返回 Err |

0.5 的源码新增用例同时覆盖有限泛型嵌套、经包装器递归布局拒绝、重复数组的零长度求值及 Copy 限制、原始取址和切片的权限／地址空间边界。没有执行合同的负面动态输入不作为 panic 用例：非法原始切片、悬空或未初始化读取不保证可捕获失败。
