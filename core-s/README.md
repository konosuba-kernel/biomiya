# Core S：源码到直接 RV64 ELF 的原型

本目录完成 B0/B1/B2/B3：Rust 种子编译器（C0）生成目标语言编译器 C1；C1、C2 继续编译同一份 VT 源码，得到全文件字节一致的 C2/C3。目标编译器自行读取源码、检查类型并生成 RV64 Linux ELF。

这是 Linux-user 下的实际自编译，尚不覆盖完整 R1a 或 SOS 内自举。下面保留种子与 B2 输入试点说明；B3 构建步骤和边界见本文后半部分。

## Rust seed 与 B2 输入试点的构建和运行

在本目录执行，需要 Rust 1.96+；首次 Cargo 构建需要依赖缓存或网络。运行目标程序需要 `qemu-riscv64` 或相应 Linux RV64 环境。

```sh
cargo build --all-features
mkdir -p build
target/debug/core-s project app/copy.build
qemu-riscv64 build/copy.elf app/copy.manifest
```

成功退出 0，`build/compiler-sources.vt` 是三个 compiler 源文件的逐字节拼接。复制程序拒绝已有输出；再次运行需换一个输出路径，或自行删除已经检查过的示例输出。`app/copy.build` 构建目标程序，`app/copy.manifest` 是目标程序运行时读取的清单，二者使用相同的行格式。

```text
core-s project MANIFEST [--root DIRECTORY]
core-s check INPUT
core-s build INPUT --entry FUNCTION -o OUTPUT
core-s asm INPUT --entry FUNCTION -o OUTPUT
```

清单路径相对 `--root`（缺省当前目录），不相对清单所在目录。`format 1`、`profile linux-user`、`entry`、`output` 各一次；`source` 至少一个，`assembly` 可重复列出不同输入。上限与拒绝规则见[合同](../docs/design/bootstrap-b2.md)。入口须为无参数、非 unsafe、返回 u32 的函数，支持模块限定名。单文件接口继续可用，例如 `build fixtures/bootstrap.vt --entry aggregate_snapshot -o build/example.elf` 的退出码为 42。

生产编译不调用 Clang、LLD 或外部汇编器，空 PATH 可构建。生成的目标程序直接使用 Linux 系统调用；种子 Rust 程序依赖宿主标准库。宿主输出采用临时文件加 rename 发布，失败保留已有输出，并检查全部源、汇编和清单的 inode 别名。这不是应对并发恶意路径替换的安全沙箱。

## Rust seed 与 B2 输入试点的实现与边界

`pipeline::compile_files` 串接每文件 parse → 模块合并／可见性 → 名字与布局 → check；`backend::machine` 生成 typed machine 记录；直接编码器和调试汇编共享这些记录。新增 `.s0` 是固定子集解析器，只供显式 C ABI 平台适配，无宏、数据伪指令或自动松弛。

B2 已实现字符串、切片、静态对象、模块、公开声明、原始指针内建和标量 `extern abi(C)`。所有 extern 都须在本次 `.s0` 输入提供，包括未调用的声明。静态对象支持 RO/RW/BSS 与 ABS64 重定位；空字符串使用非零哨兵地址；切片访问保留动态长度检查。ELF 使用 RX/R/RW 段，BSS 不膨胀文件，映像跨度上限 64 MiB。

这是旧 Core S pilot 加 B2 必要增量：整数仍仅 u8/u32/usize；比较两侧各自定型，`n == 0_u32` 合法，`n == 0` 不合法；`*kernel T` 暂作兼容写法。公开结构要求所有字段 pub，静态初始化仅支持合同列出的形式。用户枚举、完整 R1a 常量规则和其他合法但未实现的能力仍报告 I_UNSUPPORTED。

Rust seed 的整程序 AST／CFG、宿主 Vec／String／映射表及原有布局依赖处理仍在；它没有迁移为逐函数 arena。B3 的目标语言编译器另行实现了逐函数 arena。宿主单文件上限 4 MiB，其余递归／时间／内存没有全面硬额度。B2 输入试点使用固定 BSS 缓冲，清单 16 KiB、单源 64 KiB；B2 扫描器只支持 ASCII 标识符，数字词素的值及合法性检查留给后续解析。目标运行库、错误码及所有权约定见 [runtime/README.md](runtime/README.md)。

## B2 验证记录

```sh
cargo test --all-features
python3 ../docs/workflows/bootstrap-b2/clean_build.py
```

B2 交付时 **61 项测试通过**（当前完整 81 项见 B3 节）：清单单元 4、B2 CLI 4、B2 语言 7、目标运行库 6、后端 14、检查器 6、旧 CLI 4、映像 8、源码管线 6、`.s0` 2。测试需要 Clang、LLD、用户态及系统态 RISC-V QEMU；缺失或超时不会算通过。旧 CLI 测试继续对 19 个入口各执行直接编码和外部工具链两条路径。

新增覆盖实际清单／多源／空源、跨 4096 字节读边界、每次最多 7 字节的短读写及 EINTR、读写和关闭失败清理、扫描字节位置、静态数据权限、切片权限／边界、模块私有泄露、未调用 extern 缺符号、路径／inode 保护。运行库独立验收另测部分输出回收、悬空符号链接输出及最终关闭错误。

干净构建仅复制本目录源码及四个直接复用文件，离线锁定依赖后跑完整测试；空 PATH 构建得到的 ELF 与工作区种子逐字节相同，并执行目标程序核对实际复制字节。证据见 [I-r1-clean-build.json](../docs/workflows/bootstrap-b2/I-r1-clean-build.json)。

## B2 规模账本（历史基线）

[metrics-r1.json](../docs/workflows/bootstrap-b2/metrics-r1.json) 使用 cloc 2.10，记录源文件哈希与计数区间。`project.rs` 的内置测试拆入测试项，直接复用的词法／诊断／源位置计入宿主实现。

| 范围 | 物理行 | code 行 |
| --- | ---: | ---: |
| 宿主种子编译器与 CLI | 7681 | 7433 |
| 目标语言运行库 | 189 | 167 |
| 目标语言 Source／扫描器／清单 | 199 | 188 |
| 目标语言复制应用 | 52 | 48 |
| 目标平台 `.s0` | 19 | 18 |
| **手写实现合计** | **8140** | **7854** |
| 生成 Unicode 表 | 2096 | 2095 |
| Rust 测试 | 3963 | 3850 |
| `.vt` 行为输入 | 98 | 96 |

相对 B0/B1，宿主 code 从 5509 增至 7433；加入目标语言及平台适配后，手写 code 合计 7854。B2 增加了自写程序必需的能力，没有带来净缩减。Cargo 外部依赖、文档及验收脚本未算入产品实现，生成表和测试单列；不能把这个子集与旧完整编译器作等能力比例比较。

架构见 [bootstrap-b2.md](../docs/design/bootstrap-b2.md)，各节点候选、独立判定及返工记录见 [workflow.json](../docs/workflows/bootstrap-b2/workflow.json)。B3 自编译见下节；SOS 承载和系统重建仍未完成。

## B3 目标语言编译器

`bootstrap/` 包含实际用 VT 写出的编译器：输入快照与词法、声明与布局、单函数 AST、类型检查、typed IR、RV64 编码、`.s0`、ELF 和命令驱动。运行中不调用宿主编译服务。B2 的 `compiler/` 是原输入试点，继续保留，生产 B3 不通过它转发编译。

在本目录执行：

```sh
cargo build --offline --locked --all-features
mkdir -p build
target/debug/core-s project bootstrap/c1.build
qemu-riscv64 -s 1048576 build/c1.elf build bootstrap/c2.build
qemu-riscv64 -s 1048576 build/c2.elf build bootstrap/c3.build
cmp build/c2.elf build/c3.elf
```

C0 是 Rust seed，C1 是 seed 生成的 VT 编译器，C2/C3 是目标编译器生成的后两代。不同生成器可采用不同指令与栈布局，因此要求 C2/C3 全文件相等。上述输出必须尚不存在；目标编译器使用独占新建，已有输出拒绝，失败会清理自己新建的不完整产物。重复构建可选择新的 manifest output，不必删除已有可用编译器。

命令为 `bmc build MANIFEST`、`bmc check MANIFEST`；示例通过 QEMU 执行 ELF，即用 ELF 文件位置代替 bmc。check 完成前端、机器与符号检查但不发布 ELF。入口支持 `run`、`app::run` 及带 `root::` 前缀的绝对名字。清单内路径相对启动目录，输出路径不会进入 ELF。stderr 的 `B3 key=value` 统计也不会进入被比较的文件。

B3 仍是 Core S/B2 原型子集：仅 u8/u32/usize、ASCII 标识符、Linux-user；包含模块、结构/数组/切片、Result/defer/?、原始指针、静态数据和标量 extern。字符数据可以是 UTF-8。未支持的合法机制报 `I_UNSUPPORTED`，资源额度报 `I_LIMIT`，不声称完整 R1a 或 SOS 内自举。

资源使用具体定长表，单函数结束复用 AST/slot/IR/argument/cleanup arena。限制包括 4 MiB source、1 MiB assembly、32768 AST nodes、65536 IR instructions、256 KiB frame、64 MiB image/data budget，另有明确递归深度配额。数组超限诊断可能按元素个数报告 actual/limit，避免错误报告本身计算超大字节乘积；输入超限的 actual 是已观察到的容量需求下界。

B3 的语言诊断保留 FileId 和字节位置。与旧 C0 有两处已发现并明确记录的差异：旧 C0 接受重复数字分隔符 `1__2`；旧 C0 在同名局部变量存在时误解析 `root::n`。B3 拒绝前者，正确限定后者；它们列为 seed 已知缺陷，未拿错误 seed 行为作为 B3 的预期结果。其余行为对照与独立执行见 B3 测试和验收记录。

运行验证：

```sh
cargo test --offline --locked --all-features
python3 ../docs/workflows/bootstrap-b3/clean_build.py
```

干净构建脚本默认将证据保存在新建的 `/tmp/core-s-b3-evidence-*` 目录，打印其位置；也可用 `--record-dir NEW_DIRECTORY` 指定新的目录。它不覆盖历史验收记录。

B3 集成测试从独立临时树只复制声明输入，逐代运行 C1/C2/C3；运行环境 PATH 为空，通过 syscall trace 检查读取路径和禁止自读/外部代编译。还要实际编译新的行为程序、修改源运算和静态字节，以及由 C1 编译修改过诊断文本的编译器，验证新编译器行为确实改变。

最终实测、源文件哈希、分项规模、独立验收和历次返工见 `docs/workflows/bootstrap-b3/`。VT 部分文件一行包含多个语句，行数受排版影响，须同时阅读 source bytes、完整模块范围及行为边界，不能直接把与 Rust 的行数比当作复杂度缩减比例。目标编译器、保留的 Rust seed、运行库、汇编适配、生成 Unicode、测试与验收工具分开记账。

### B3 本次实测（H3 / L2 / F3 / M3）

完整回归共 81 项。C2/C3 均为 1,062,968 字节，完整文件 SHA-256 均为 `8444edaad7dd0911dc9c0363965067c13e3b02209fbe6f8f70e0f1457296b33a`。

| 项目 | 实测 | 配额/说明 |
| --- | ---: | --- |
| 目标编译器源码 | 181,404 字节 / 4,040 物理行 | 11 个 VT 模块；排版较密 |
| 目标运行库 | 6,778 字节 / 189 物理行 | 2 个 VT 模块 |
| Linux 汇编适配 | 345 字节 / 19 物理行 | 1 个 `.s0` |
| ELF RW 段内存 | 52,553,432 字节 | 包含全部静态定长表与 BSS，约 50.12 MiB |
| ELF 映像跨度 | 53,413,592 字节 | 小于 64 MiB |
| 自编译单函数 frame 最大值 | 26,384 字节 | 256 KiB |
| 自编译单函数 AST / IR 最大项数 | 867 / 1,633 | 32,768 / 65,536 |
| QEMU guest 栈配置 | 1,048,576 字节 | 固定点与扰动测试在此配置下运行 |

这里的 RW 段内存和映像跨度是 ELF 静态布局，不能等同于进程峰值 RSS；单函数 frame 也不是整个调用链的栈峰值。保留的 Rust seed 另有 7,681 物理行，生成 Unicode 表、测试与验收工具不混入目标编译器数字。分项文件及哈希见 [metrics.json](../docs/workflows/bootstrap-b3/metrics.json)，干净构建见 [I-r2-clean/I-clean-build.json](../docs/workflows/bootstrap-b3/I-r2-clean/I-clean-build.json)，独立验收状态见 [workflow.json](../docs/workflows/bootstrap-b3/workflow.json)。
