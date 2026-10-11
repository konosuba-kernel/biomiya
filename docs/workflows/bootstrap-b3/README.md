# B3 自编译交付记录

需求：实现 bootstrap-v1 的 B3，Linux-user 下由 VT 编译器编译自己的 VT 源码。节点状态与独立结论以 [workflow.json](workflow.json) 及各节点最新 `*-review.json` 为准；合同中“候选”的历史标题不代表当前状态。

依赖图为 H → L → F，H → M，L + F + M → I；当前集成 I2 输入 H3 / L2 / F3 / M3。I1 的入口 `root::` 前缀兼容缺陷在 I2 修复；I1 产物证据与失败判定完整保留。H/L/F/M 的失败候选及返工记录保留，不能将旧候选通过记录用于新版本。

## 可复现证据

- [I-r2-tests.txt](I-r2-tests.txt)：当前工作区完整 81 项回归。
- [clean_build.py](clean_build.py)、[I-r2-clean/I-clean-tests.txt](I-r2-clean/I-clean-tests.txt)、[I-r2-clean/I-clean-build.json](I-r2-clean/I-clean-build.json)：只复制声明源码的独立树，离线锁定依赖重建 Rust seed，运行 81 项回归，然后按三个正式 manifest 生成 C1/C2/C3。
- [I-r2-workspace-build.json](I-r2-workspace-build.json)：工作区保留的 `core-s/build/c1.elf`、`c2.elf`、`c3.elf`。C2/C3 完整字节一致，且与干净构建一致。
- [I-baseline-check.json](I-baseline-check.json)：B2 产品源码未改变；旧文件仅 README 更新。
- [metrics.json](metrics.json)：目标编译器、运行库、汇编、保留的 seed、生成表、测试、验证工具分开计数；密集排版下行数不能证明等比例降低了复杂度。

端到端测试 `core-s/tests/b3.rs` 既检查固定点，也检查 C0/C1/C2/C3 实际翻译新程序的行为、用户源码变化、编译器诊断源码变化、多模块与汇编、错误拒绝、输入保护。目标编译执行采用空 PATH 和 1 MiB guest 栈；逐代干净目录与 syscall trace 检查禁止读自身 ELF、执行外部编译器和读取未声明文件。

## 结果与边界

作者构建：C1 768,056 字节；C2/C3 1,062,968 字节，SHA-256 为 `8444edaad7dd0911dc9c0363965067c13e3b02209fbe6f8f70e0f1457296b33a`。独立验收另记于 I 最新 review。I1 的工作区产物另存 `core-s/build/b3-i1/`；当前 `build/c1.elf` 等是 I2。

目标编译器 11 个 VT 模块，181,404 字节 / 4,040 物理行；运行库 6,778 字节，汇编 345 字节。全部静态表计入 ELF RW 段：52,553,432 字节，BSS 52,354,720 字节；映像跨度 53,413,592 字节。自编译最大单函数 frame 26,384 字节、AST 867 项、IR 1,633 项。这些是静态布局和编译器计数，不是峰值 RSS 或整个调用链的栈峰值。

仍是 Core S/B2 语言子集和 Linux-user profile，不等同于完整 R1a、SOS 内自举或取消 Rust seed。ASCII 标识符、u8/u32/usize 与显式资源配额保持不变。已知 seed 差异：C0 错收 `1__2`、同名局部变量误遮蔽限定 `root::n`；B3 按语言要求拒绝前者、正确解析后者，未修改旧 seed。

独立 I1 探针脚本的 `.txt` 快照在 metrics 的 `I1_independent_probe_snapshots` 单列，不伪装成产品代码；I2 复核的命令与结果以 I2 review 为准。
