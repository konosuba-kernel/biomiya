# N1 r1 实施候选（待独立验收）

输入：N0 r1 已验收；B3 `1c0f143`；旧 C2 seed SHA-256 为 `8444edaad7dd0911dc9c0363965067c13e3b02209fbe6f8f70e0f1457296b33a`。
作者：`/root/b31_frontend`。本报告不构成独立通过判定。

## 改动

- 11 个生产 bootstrap VT 模块展开块、语句和记录字段；Result 泛型参数保留在同一行。没有新增模块、改变 manifest、Rust seed、语言能力或旧 B3 证据。
- `model.vt` 的 `pub const` 命名 Type/Decl/Token kind、AST tag、IR op、二元/一元运算、诊断类别、声明/类型/参数槽 flags，以及 Result tag 和符号类别。保留真实机器编码、字节偏移、计数/容量与零哨兵。类型范围、比较范围和终结指令范围保持原编号关系。
- `model.vt` 注释记录 Type/Decl/Slot/Inst 的字段域、缺省值和寿命；`parse.vt` 注释覆盖每个 Node tag 的字段含义。
- `ir.vt` 新增 `copy_to`、`bounds`、`call`、`return_value`、`panic_`、`unary`；checker 通过这些语义构造发出指令，不再直接填充通用 `emit` 的七个位置。保留后端原有完整 IR 校验。

## 可重跑的阶段隔离

以下脚本从仓库根运行：

1. `python3 docs/workflows/bootstrap-b31/N1-format.py --baseline` 重建仅排版阶段。输出应等于 `N1-format-evidence.json`；全部 11 个文件逐 token 一致，包括字符串和注释内容。`--baseline` 不改产品源码；不要对最终源码运行无参数模式。
2. `python3 docs/workflows/bootstrap-b31/N1-symbol-check.py` 校验归档的 `N1-symbolic-snapshot.tar.gz`。去掉新增 const 声明，将引用逆展开为原数值，去掉数值类型后缀，再逐 token 对照旧 B3。新增常量均为 u32；类型正确性另外由 C0 和目标编译器验证。
3. `python3 docs/workflows/bootstrap-b31/N1-final-check.py` 先严格比对六个新 IR wrapper 函数体，再将其调用还原为原 emit/slot，逆展开最终常量并逐 token 对照旧 B3。全部相等，覆盖最终代码相对基线的改动边界。该机械核对是实施证据，不代替独立验收。
4. `N1-generic-format-evidence.json` 单列最终 Result 泛型换行修正；仅 input/main 空白改变，token 完全一致。

## 执行验证

工作目录为 `core-s`：

- `cargo test --offline --locked --all-features`：两轮均 **81 passed / 0 failed / 0 ignored**。`N1-final-cargo-tests.log` 是最终语义版本的复验，随后只有上文已证明 token 一致的 Result 空白修正。没有改变或放宽测试。
- 完整回归含 C0→C1→C2→C3 全文件固定点、源码运算/静态字节/诊断文本扰动、空 PATH 与 syscall trace 的无宿主代编译核对、模块/extern、诊断、布局、聚合快照、清理和非法 IR 反例。
- `qemu-riscv64 -s 1048576 build/n1-seed-executable.elf build build/n1-d1.build`：旧 B3 seed 成功编译最终语义源码。
- `qemu-riscv64 -s 1048576 build/n1-d1.elf build build/n1-d2.build`：新编译器再次自编译；D1/D2 全文件相等。
- `qemu-riscv64 -s 1048576 build/n1-d2.elf build build/n1-formatted.build`：编译最终排版源码；与 D1/D2 全文件比较，结果记录在 `N1-validation.json`。
- 上述临时 build manifest 仅将现有 `bootstrap/c2.build` 的 output 改为相应新文件名；没有改仓库 manifest。运行参数保持 1 MiB guest 栈。
- `git diff --check`：通过。

首次直接执行基线 seed 时，文件模式为 0644，QEMU 报 Exec format error。使用字节相同的副本 `n1-seed-executable.elf` 并赋执行位后成功；原 seed 内容和模式未改变。失败日志保留在 `N1-old-seed.log`，重试与最终构建分别记录，不将失败算作成功。

本节点未做额外全 clean；最终 N5 按合同执行干净重建与完整集成。本节点不声称 N2 状态所有权、N3 语义抽取或 N4 分析已实现。

## 规模

生产 bootstrap 仍为 11 个 VT 文件。最终 7,681 物理行、251,657 字节；原始基线为 4,040 行、181,404 字节。增长主要来自展开排版、名称和字段合同；不将行数变化解释为复杂度变化。运行库、s0 和保留 Rust seed 未修改。ELF/静态映像度量和逐文件哈希见候选与 `N1-validation.json`。
