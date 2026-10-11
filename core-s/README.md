# Core S B3.1

这是当前编译器。Rust 种子 C0 编译 VT 编译器得到 C1，再由目标编译器生成完整文件字节一致的 C2/C3。编译器在 RV64 Linux-user 中自行读取源码、检查类型并生成 ELF，不调用宿主编译服务。尚未完成完整 R1a 或 SOS 内自举。

## 目录

| 目录 | 内容 |
| --- | --- |
| `src/` | Rust 种子；宿主可执行文件为 `target/debug/core-s` |
| `bootstrap/` | 当前 VT 编译器；驱动入口 `main.vt` |
| `runtime/` | Linux 系统调用与字节切片支持 |
| `tests/` | Rust 回归测试；`tests/fixtures/b2-input/` 是保留的早期输入处理夹具 |
| `fixtures/` | 基础语言行为输入 |
| `tools/verify.py` | 当前目录布局的干净源码构建与自编译验证 |
| `build/` | 本地产物，Git 忽略；不是源码或版本依据 |

原来的 `app/` 和 `compiler/` 已迁入测试夹具；它们不参与自编译。夹具保留是为了继续验证短 I/O、EINTR、词法错误和失败清理，见[夹具说明](tests/fixtures/b2-input/README.md)。

## 构建与运行

以下命令均在 `core-s/` 执行。需要 Rust 1.96+ 和 `qemu-riscv64`；离线 Cargo 构建要求依赖已缓存。完整测试另需要 Clang、LLD 和系统态 RISC-V QEMU，工具缺失不会算通过。

```sh
cargo build --offline --locked --all-features
mkdir -p build
target/debug/core-s project bootstrap/c1.build
qemu-riscv64 -s 1048576 build/c1.elf build bootstrap/c2.build
qemu-riscv64 -s 1048576 build/c2.elf build bootstrap/c3.build
cmp build/c2.elf build/c3.elf
```

输出必须尚不存在。重复执行时可修改清单中的 output 为新路径；编译器拒绝覆盖已有文件。清单路径相对启动目录，输出路径不进入 ELF。

本地已验证的 B3.1 产物为 `build/b31-final.elf`，SHA-256 为 `0793011738def41baca6387a7a6917c690ddbe15ac812340b009866c8aa56ffc`。它不随 Git 分发，可通过上述步骤重建。检查自身源码与可选分析：

```sh
qemu-riscv64 -s 1048576 build/b31-final.elf check bootstrap/c1.build
qemu-riscv64 -s 1048576 build/b31-final.elf check bootstrap/c1.build --blocking bootstrap/pure.blocking
```

目标 CLI 为 `build MANIFEST [--blocking POLICY]` 或 `check MANIFEST [--blocking POLICY]`。`check` 完成前端、机器和符号检查但不发布 ELF。Rust 种子的入口是 `project MANIFEST [--root DIRECTORY]`，另提供单文件 `check` / `build` / `asm`，没有 Blocking 命令。

## 支持范围与参考文档

当前支持 bool/unit/u8/u32/usize、模块与可见性、具体结构/数组/切片、原始指针、常量与静态数据、字符串、普通函数与标量 C extern、if/while/loop、浅层 match、Result/defer/?。整数 checked 与 wrapping 运算分开，具有运行时算术与索引检查。标识符仅 ASCII，字符串和注释可包含 UTF-8。

不支持用户泛型、用户枚举、闭包、间接调用、新整数宽度或完整 R1a。比较两侧各自定型，例如 `n == 0_u32` 与 `n == 0` 不一定等价。公开结构要求全部字段 pub，静态初始化形式有限，extern 必须由本次 `.s0` 输入提供。资源使用具体定长表与逐函数 arena，资源耗尽报告 `I_LIMIT`。

目前没有完整、统一且与 B3.1 逐项对应的语言参考手册。可分别查阅：

- [B3 实现合同](../docs/design/bootstrap-b3.md)：已验收实现的语言子集、ABI 和资源边界；历史候选中的内部记录以当前源码为准。
- [B3.1 改进合同](../docs/design/bootstrap-b31.md)：状态、纯规则和分析边界。
- [Blocking 使用说明](bootstrap/blocking.md)：配置与 Verified / Violation / Unknown 的保证范围。
- [运行支持](runtime/README.md)：Linux 适配与 I/O 合同。

[0.5 参考](../docs/reference.md) 对应旧实现，[R1a 草案](../docs/design/reference-next.md) 是未来设计，均不能直接视为当前支持声明。Rust seed 已知仍接受重复数字分隔符 `1__2`，并在局部同名变量下错误解析 `root::n`；目标编译器已拒绝前者并正确处理后者。

## 源码阅读与状态边界

先读 `model.vt` 的记录合同和命名判别值，再读 `rules.vt` 的整数/布局规则、`main.vt` 的阶段顺序，最后沿 `parse → check/ir → rv64/image` 阅读。现有表达式与指令分派仍有过深嵌套，是待改进的可读性问题。

扫描游标与诊断 Span 分离，AST 保存所属文件。check 负责函数状态初始化；完成后的语义 Slot/Inst/Arg 对分析和后端只读。rv64 使用自己的帧偏移与标签工作表，blocking 只保存稳定 DeclId 调用摘要。类型条目完成后只读，后续仍可追加新类型。静态初始化的 `validate_static_zero` 完整验证所有子项，包括 `[expr;0]` 中的表达式，再决定是否走零填充路径。

Blocking 是具体的直接调用 may-block 分析；缺少外部合同不能通过。它不证明终止性、IRQ 或并发安全，也不提供通用插件 ABI。开启分析不改变产物字节，所有要求通过后才允许发布 ELF。

## 验证与规模

```sh
cargo test --offline --locked --all-features
python3 tools/verify.py
```

第二条在仅复制声明源码的临时树中重建 Rust 种子、运行完整回归并执行 C1/C2/C3 自编译。目标运行时 PATH 为空，guest 栈为 1 MiB；C3 开启六个规则入口的 Blocking 检查并与 C2 逐字节比较。日志、源文件哈希和验证后的 `compiler.elf` 保存在打印出的新目录中，也可指定 `--record-dir NEW_DIRECTORY`。完整回归保留 B3.1 的全部 91 项测试。

B3.1 基线的 13 个生产 VT 模块共 269,906 字节、8,249 物理行（7,843 code 行）；其中 Blocking 为 354 物理行。运行库 189 行、平台 `.s0` 19 行、Rust seed 7,681 行、生成 Unicode 2,096 行分别记账。排版展开导致的行数增长不等于语义规模增长。历史逐文件账本见 [metrics.json](../docs/workflows/bootstrap-b31/metrics.json)；其中旧试点路径按当时布局记录，当前已迁入测试夹具。

基线 ELF 为 1,108,040 字节，RW 段 53,118,748 字节、映像跨度 54,023,964 字节，均低于 64 MiB；自编译最大单函数 frame 为 26,384 字节。它们是静态布局与单帧统计，不是进程峰值 RSS 或调用链栈峰值。

旧 B3 迁移 seed 保留为本地 `build/b31-seed.elf`，也可在 Git `1c0f143` 重建。冻结的迁移证据与各阶段审查见 [历史验收索引](../docs/workflows/README.md)；历史 clean-build/metrics 脚本须在其对应版本运行，当前使用 `tools/verify.py`。
