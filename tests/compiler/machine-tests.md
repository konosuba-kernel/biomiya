# Q 的 IR / 机器回归

`machine-cases.json` 的 83 例使用 Q 的真实模块；其中独立 pure-rules fixture 只依赖 model、rules；另有 owner-cases.json 的 16 项阶段与写入边界检查。
`layout.vt` 只为手写测试 Type 提供已完成布局和 pointer 构造；它不代替生产 types，
rv64 自己的有限布局、大小和对齐校验仍然执行。
`ast-cases.json` 的 6 例另用真实 parse、types、init，不使用这个 layout 测试模块。

从 core-s 目录运行：

```sh
python3 tools/check_q_machine.py --compiler build/snext-p3-r1/compiler.elf --record-dir build/q-machine-e0-fresh
```

正式验收把 `--compiler` 换成 P4 固定点的 E3。记录目录必须不存在。
工具仅用指定编译器和绝对路径 QEMU；guest PATH 为空，栈为 1 MiB。
每个不同 manifest 编译一次，各运行例有独立目录；含 AST 与 owner 总计 105 例。生成的每个运行 ELF 都显式 chmod，
检查 RV64 ELF 身份及 64 MiB 内存限制。记录源和工具 SHA、源 capsule、编译/执行命令、
输出、ELF SHA、内存、耗时及最终输入身份复核。`passed` 只是机械检查结果，
`independently_accepted` 始终为 false，由独立验收判断。

## 具体保证

- `simple`、`aggregate`：退出 42；后者覆盖 10 参数、聚合隐藏返回指针、栈上传参、
  参数副本及 snapshot 隔离，并在相邻函数间复用 IR 和 frame 表。
- `operators`：在目标程序中检查全部 19 个二元和 3 个一元操作；
  `overflow`、`divzero`、`shift` 验证 panic 101，`wrap` 验证 0。
- 驱动每次启动断言 `Inst=48`、`Instruction=32`、`SlotId=4`、`LabelId=4`、
  `ArgumentRange=8`、`Destination=8`，并在后端前后检查计数和 revision 不变。
- `labels_high` 接受标签 65,536；`arguments_high` 接受实参表第 65,536 项，
  随后空 range 的首位置 65,537 仍合法。相应超限例和 frame 超限返回 I_LIMIT。
- 拒绝错误 Copy/Load/Return、调用实参个数/返回位置/声明、非法槽、缺少/重复标签、
  缺少终结、终结后指令、Empty 指令、索引高位混入世代。
- 拒绝未 verify 发射、二次发射、finish 后修改及函数重置后的 SlotId、LabelId、
  ArgumentRange。错误例不发布 out.elf。

## 冻结 B3 七个机器测试身份的完整迁移

| B3 测试身份 | Q 的实际回归 |
| --- | --- |
| target_ir_aggregate_abi_snapshot_abs64_bss_and_checked_panic | aggregate、machine-abs64-bss（原退出 97，RW file=8 / memory=12,288）、算术 trap 与 wrap |
| target_constants_match_rust_and_independent_assembler | 原 13 个 u64 向量；整个 Q ELF 等于冻结 Rust image 输出；运行 stdout 精确等于 104 字节向量及 LLVM 独立程序 |
| target_s0_all_operations_far_fixups_match_independent_bytes | 真实 Q s0 解析全部旧操作，1,700 个 nop 制造远跳；整个 ELF 等于 Rust S0/encode/image，text 字节等于 LLVM 独立汇编 |
| target_rejects_invalid_ir_and_link_data_before_output | 原类型、权限、调用、返回、标签、frame 错误；duplicate/absout/absunder/overlap/jumpdata/undefined 六项链接拒绝 |
| target_requires_label_one_and_finite_value_layouts | nolabel/label_two/badcycle/badmutualcycle 拒绝；selfpointer/selfslice 接受并运行 |
| target_preserves_semantic_ir_and_reuses_sparse_label_ranges | 私有只读表和 revision 快照；65536→2→65536 三函数、第三函数 yes/no 缺失不得被第一函数标签满足，重复/无终结/终结后/隐式穿透均拒绝 |
| target_s0_rejects_syntax_ranges_and_unsupported_grammar | 原 13 条寄存器/立即数/语法/重复标签错误的精确来源位置，以及 4 条不支持语法 |

旧 raw LabelId(65537) 现在不能合法构造；对应容量例在 owner 分配边界提前 I_LIMIT。
此外移入 `tests/backend.rs` 的真实重叠复制合同：目标程序执行两个方向的重叠 memmove、
超过 4 KiB 的 frame 地址以及零大小聚合/unit 的参数返回，退出 6。

## 原测试合同的补充

以下十例保留原 79 例，补足原 Rust / B3 测试的独立断言；新 revision 必须重新执行，
不能把旧 79 例账本当作它们的通过证据。

| 原测试身份 | 新用例与实际断言 |
| --- | --- |
| pure_rules_use_only_explicit_scalar_and_layout_inputs | pure-rules-explicit-inputs：移植完整规则检查；包括非法操作、unit 比较、usize 加法溢出、align 0/3/溢出；不建立 model 类型表 |
| far_control_flow_calls_addresses_and_scratch_contract_execute | machine-far-scratch：1,700 个不可达指令隔开函数和分支；执行四种条件的真/假分支、远 call/address/jump；检查 x5/x6/x7/x28/x29/x30 在调用后及远跳之后仍保留原值，目标退出 0 |
| elf_headers_segments_entry_and_padding_are_exact | machine-page-0/1021/1022/1023/1024：目标运行退出 0；宿主独立构造整 ELF 期望，比较所有 ELF / program header 字段、text、零填充、空数据段和文件末尾；每例另起生成进程要求整个 ELF 一致 |
| hidden_result_unit_omission_and_over_eight_positions_follow_lp64 | machine-raw-abi：12个参数含2个unit；raw callee在prologue之前检查a0–a7及两个栈位置，u8零扩展、u32符号扩展、16字节栈对齐；目标退出42 |
| text_capacity_is_checked_before_output_allocation | machine-text-capacity 正好填满 Q 的 8 MiB text 并运行；machine-text-overflow 再追加 4 字节，要求精确 I_LIMIT / actual / capacity，且 out.elf 不存在 |

文本限制有明确变化：原 Rust 编码器限制 **64 MiB**，Q 的实际 text 限制为 **8 MiB**。
这里继承“边界成功、追加指令在发布输出前拒绝”的合同，并没有声称沿用相同限额。
页布局检查的期望从固定 RV64 退出序列及 ELF 规范字段独立计算，不调用 Q 或 Rust image 代算。

`tools/q_machine_oracle.rs` 只组装原常量 machine::Program 或读取 S0，
直接复用冻结 `src/machine.rs`、`encode.rs`、`image.rs`、`s0.rs`。
小型本地 Span/Diagnostic carrier 只去除与机器无关的 serde 依赖。
runner 记录并快照这些源，以 rustc 构建宿主 oracle；它从不读 VT，
也从不生成 Q 编译器或被测驱动。clang/lld 同样只把独立参考汇编变成比较用程序。
所有被测 VT（包括 Q backend、IR、S0 和 AST）的产物仍由 `--compiler` 唯一生成。

## 迁移成本

在移植到 S-next 且已简化静态 zero 的 Q 基础版上比较本节点四文件：

| 文件 | 之前行数 | 之后行数 | 字节变化 |
| --- | ---: | ---: | ---: |
| ir.vt | 233 | 797 | +12,222 |
| model.vt | 992 | 764 | -6,223 |
| rv64.vt | 1,169 | 1,314 | +4,730 |
| blocking.vt | 346 | 350 | +111 |
| 合计 | 2,740 | 3,225 | +10,840 |

净增 **485 行**，超过早期 100–300 行估计。包括具体载荷、私有句柄、世代与封口合同、
存储拥有者和闭合操作符分派；不包括 checker/main 配套修改、AST 节点、平台 I/O，
也不把本目录测试与工具算入生产编译器。这张表是 P3 当时的迁移成本；后续 R-ID 已将稳定 NameId/TypeId 名义化，Decl/Symbol 仍是受检 u32。
R-Owner 又将 input/image 表迁回各自拥有者，最新规模以 cost.json 的完整生产账本为准。

## R-Owner 写入边界

owner-cases.json 的 16 项使用真实 input/image/model：路径与源码从调用者可写buffer复制、冻结后追加拒绝、header只能完成一次、header完成后拒绝import、对象写入/复制范围、重复symbol定义、文本阶段不能重启、patch范围/对齐以及finish后的各类写入拒绝。详情见 ../ownership.md。

递归类型机器fixture已随R-ID改用pending_record与complete_type建立合法名义句柄，再让后端判定值递归非法、指针/切片递归合法；没有为了测试提供公共raw TypeId构造器。
