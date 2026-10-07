# 构建、运行与验收

编译器以 reference 0.5、rv64-core-v1 为基线。测试结果按 [workflow](workflow.md) 的分层门槛解释，完整验收报告由本文末列出的命令生成；版本号本身不表示全部条款已经验收。

## 宿主与依赖

首个参考宿主为 Linux x86_64。版本记录在仓库根的 `toolchain-lock.json`：Rust 1.96.0、Clang／LLD 23.1.1、QEMU 11.1.1。版本检查针对当前配置的 Rust 工具链，不通过目录配置自动下载另一个同版本安装；使用 rustup 或系统 Rust 均需通过此检查。Python 3.9 及以上用于验收驱动，脚本只使用标准库。Cargo.lock 固定 Rust 依赖，Unicode 15.1 数据在 vendor 中维护。

```sh
python3 tools/doctor.py
cargo build --locked
```

Clang 必须包含 RISC-V 集成汇编器，LLD 必须能链接 `elf64lriscv`；工具名分别为 `clang`、`ld.lld`、`qemu-system-riscv64`。驱动不使用系统默认目标、不自动安装软件。其他版本可以用于开发，但其结果不等于锁定参考环境的复现验收。

对象及最终 ELF 必须声明相容的 `.riscv.attributes`，且不得设置 RVC、浮点 ABI、RVE、TSO 等不相容标志。首版接受 RV64I 2.0／2.1、M 2.0、A 2.0／2.1、Zicsr 2.0、Zifencei 2.0，以及 M／A 隐含的 Zmmul、Zaamo、Zalrsc 1.0；额外扩展或未知版本拒绝为 `E_LINK`。这项检查验证生产者声明的需求，不反汇编证明手写对象中的每条指令；外部对象必须如实声明其 ISA 要求。

## 检查与编译

```sh
target/debug/biomiya check examples/hello.vt
target/debug/biomiya build --platform qemu-virt-core-v1 -o build/hello.elf examples/hello.vt
qemu-system-riscv64 -machine virt -accel tcg -cpu rv64 -m 128M -smp 1 -bios none -display none -monitor none -serial stdio -kernel build/hello.elf
```

参考平台从随仓库提供的 M-mode 启动代码进入 S-mode；程序返回后平台报告结果并停止 QEMU。示例使用平台固定 UART 地址。实际平台合同和限制见 [运行时说明](../runtime/rv64/README.md)。

源文件必须显式列出，多个文件合并模块后检查；没有目录搜寻或隐式 main。`check` 不链接外部符号。`build --emit asm` 只输出汇编，`--emit obj` 输出对象；默认 `--emit elf` 要求 `--platform` 或 `--link-script`。可重复 `--object path.o` 添加外部对象／静态库。生产参考平台需要已定义的 `kernel_main`；测试用 `--test test` 生成普通 biomiya ABI 适配器。

只有一个 `@trap_entry` 时驱动自动为参考平台生成入口适配器；存在多个时用 `--trap-entry 模块::函数` 选择。普通源调用仍不得调用特殊函数。自有平台可以用声明允许的 `@export` 名字连接自己的入口。

`--diagnostics json` 输出单个 JSON 结果；进程返回 0 表示请求成功，1 表示规范／链接拒绝，2 表示使用、工具或实现错误。`I_UNSUPPORTED` 与 `I_LIMIT` 不属于规范负例通过。`--report path.json` 保存带源码／平台摘要、工具命令、版本、目标依赖及栈帧事实的报告。

`--emit asm` 在符号或自定义节需要字节恢复时同时产生 `output.s.linkage.json`。这保留 LLVM 汇编文本不能直接表示的合法导出名／节名，包括非 UTF-8 字节。用 `biomiya assemble -o output.o output.s` 读取同名伴随文件并恢复 ELF 名字；伴随文件改名时显式加 `--linkage path.json`。直接输出 `obj`／`elf` 时驱动自动完成恢复。移动汇编产物时须一起保留该文件。

`--dump-ir path` 保留调试用 HIR／MIR，格式不承诺跨版本兼容。`--keep-temporaries` 保留临时汇编、对象、链接 map，目录在报告中。`--clang`、`--linker` 覆盖工具路径。`--sysroot` 或 `BIOMIYA_SYSROOT` 指向包含 `runtime/` 与 `platforms/` 的仓库／安装数据目录。

Extend 默认都报告 NotEnabled。当前没有真实验证器，显式 `--verify ownership` 等请求报告不可用。测试中的假分析只验证接口，不提供所有权、借用或内核安全保证。

## 验收入口

```sh
cargo test --locked --all-targets
python3 tools/conformance.py --json build/conformance.json
python3 tools/test_audit.py --json build/audit.json
python3 tools/test_integration.py --json build/integration.json
python3 tools/test_runtime.py --json
python3 tools/accept.py --stage full --json build/acceptance.json
```

Rust 的后端和降低测试也会调用真实 Clang／LLD／QEMU。需要这些工具时缺失就失败，不静默跳过。Conformance runner 对 9 个 Extend 用例仅执行 Core 接受检查并标明真实验证 NotProvided。

`accept --stage core` 和 `--stage platform` 仅执行对应阶段；完整测试门槛还要求条款覆盖审计。每项结果保留命令、诊断或 QEMU 协议。PASS 中的整数由主机对照预期，PANIC、FAULT、无结果及超时相互区分。不能将有限次并发执行未出错当作原子内存模型证明。

## 资源与平台边界

首版对每次编译序列化的静态映像总量设 64 MiB 预算，超过返回 `I_LIMIT`；这个实现资源限制不是规范类型错误。源码位置使用 32 位字节偏移，单文件不能超过该地址空间。普通局部对象仍按 RV64 布局计算；实际可运行尺寸和调用深度受平台 RAM 与栈约束。当前后端为局部／临时分配栈槽，不提供优化器或栈界限证明。

`qemu-virt-core-v1` 支持最多四个 hart，各有 128 KiB 普通栈及 64 KiB 状态／陷阱区域。它以 bare 模式运行，ELF 权限标志并非硬件页保护；陷阱处理不得睡眠、调度或主动开启 SIE，嵌套故障可靠进入 panic。生产内核需自行提供与其映射、调度及设备相符的平台配置。

直接使用 Rust MIR API 的聚合相等后备发射路径有 100,000 次类型／分量访问预算，超过返回 `I_LIMIT`；普通源程序降低使用循环，不受此发射展开预算限制。未实现的真实 Extend 验证器、稳定外部 IR 协议、额外目标、优化器、自举、LSP 和包管理器不属于本轮交付。
