# RV64 直接映像实验

这个实验验证[最小自举实现合同](../../docs/design/bootstrap-v1.md)中的机器编码、地址回填和 ELF 输出路径。它用 Python 标准库构造一个手写机器程序；没有语言前端或 SOS 加载器，不代表编译器已经自举。

在仓库根目录运行：

```sh
python3 experiments/bootstrap-image/probe.py
```

需要 Python 3.8+、`qemu-riscv64`、`readelf`、支持 RISC-V 的 `clang` 和 `ld.lld` 在 PATH 中。不要使用 Python 的 `-O`，检查使用了 assert。

被测 `out/direct.elf` 完全由 Python 写出。Clang/LLD 只生成另一份参考映像，随后分别比较 `.text`、`.rodata`、`.data` 的全部字节，不参与被测文件的生成。运行适配使用 Linux 的 write/exit 系统调用。

执行检查包括：

- 超过 4 KiB 的前向调用、条件远跳和反向跳转；AUIPC 高低立即数进位及越界拒绝。
- 静态 ABS64 指针和初始化数据；跨页 BSS 首尾的零值。
- QEMU 用户态退出状态和完整标准输出。
- 与独立汇编／链接路径的三个初始化段逐字节比较。

2026-10-10 实际运行通过：ELF 文件 16400 字节，text 5416、rodata 3041、data 16、bss 4096 字节；13 个 PCREL 回填、1 个 ABS64 回填；QEMU 退出 0。被测映像 SHA-256：

```text
0b4d4f7e77ebe1f5044173d63d691977969bfe3865777aa5bda81be83ee68a0e
```

产物在被 Git 忽略的 `out/` 中：`direct.elf`、`readelf.txt`、参考汇编／链接文件和 `report.json`。每次运行先删除旧报告，仅全部通过后写出新报告；工具缺失或检查失败会以非零状态退出。

它尚未验证其余 RV64 指令编码、类型检查、函数调用约定的完整实现、SOS 装载与权限、磁盘持久化或源程序自编译。下一步应把已有前端产生的 IR 接入相同输出路线，再验证源程序行为。
