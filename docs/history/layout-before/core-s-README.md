# Core S

当前开发实现是 **S-next 0.1**，源码在 [next/q/](next/q/)。它已在 RV64 Linux-user 完成 E1=E2=E3 的完整自编译固定点；2026-10-14 K0工具链及首条内核路径的源码扰动、回归与独立验收状态见 [本轮交付记录](../../suba/docs/workflows/k0/DELIVERY.md)，此前架构交付见 [原交付记录](next/delivery.md)。Linux-user 自举不代表 suba 内自举。

## 入口

| 目录 | 职责 |
| --- | --- |
| `next/q/` | 当前维护编译器 Q：具体 AST/IR、前端、RV64、汇编、ELF、Blocking |
| `next/q/runtime/` | Q 的 Linux 支持及具体 I/O 结果枚举 |
| `next/tests/`、`next/q/tests/` | 新规则与目标内部合同验证 |
| `next/system/` | 页分配/页表、文件回滚、设备/双 hart 原子样本与有界监控层 |
| `next/*.vt` | 一次性过渡 P，使用 B3.1 实现语言，接受 S-next |
| `bootstrap/`、`runtime/` | 冻结 B3.1 VT 编译器和运行支持 B |
| `src/`、`tests/`、`fixtures/` | 冻结 Rust 种子、旧回归与夹具；不追赶 Q 的新语义 |
| `tools/` | 引导、固定点、语言、机器、输入、参考和成本验证工具 |
| `build/` | 忽略的本地产物；身份由内容哈希识别 |

语言使用者阅读 [S-next 0.1 版本参考](docs/reference/s-next-0.1.md)。历史 B3.1 的接受集仍由 [冻结参考](docs/reference/b3.1.md) 描述；版本索引见 [reference/README](docs/reference/README.md)。

## 构建

以下在 `core-s/` 执行。已有目标 B 为 `build/b31-refactored.elf`，SHA-256 `90c9523de43bebbd0a6932e74abc8cd670b60478c3b4f0fc1eff74fbf4cfb686`。没有 B 时先执行：

```sh
python3 tools/verify.py --record-dir build/rebuilt-b
```

该命令从冻结 Rust/VT 输入重建 B，要求 Rust/Cargo 缓存、qemu-riscv64 及原目标检查所需 LLVM 工具。重建的 `compiler.elf` 应具有上述身份；在后续命令用它的路径替换 `--seed`。

```sh
python3 tools/check_next.py --stage P3 --seed build/b31-refactored.elf --record-dir build/p3-new
python3 tools/selfhost_next.py --p3-record build/p3-new --seed build/b31-refactored.elf --q-manifest next/q/compiler.build --record-dir build/p4-new
python3 tools/check_q.py --compiler build/p4-new/e3.elf --record-dir build/language-new
```

证据目录和目标输出必须是新路径。完整机器、输入、参考和系统复验命令见 [Q 阅读与验证入口](next/q/README.md)。直接运行已有 Q：

```sh
qemu-riscv64 -s 1048576 build/k0-p4-r2/e3.elf check next/q/compiler.build
```

目标 CLI：`build|check MANIFEST [--blocking POLICY]`。清单路径相对进程工作目录，build 拒绝覆盖文件，check 不发布。Rust 种子的 CLI 为 `project MANIFEST [--root DIRECTORY]`，不可混用。

## 当前结果与边界

S-next 支持具体结构/枚举、单向局部定型、显式 make/zero、私有字段、调用式 defer、原始指针/切片、有限常量与静态初始化。固定 Result、问号、块式 defer、隐式根模块和 root 特殊路径已删除。仍无用户泛型、函数指针、闭包、额外整数宽度、完整 C ABI 或通用优化框架。

当前固定点 ELF 为 1,036,432 字节，SHA-256 `049c6e094ccd044fba3b5d357583013b77f0e8e9a98d431ad919e02b0cda2144`。Q 生产源码 **11,307 行**，高于 8,000 行原型预算；不能把已自举等同于已经简洁。本轮[维护记录](next/q/k0-2026-10-14.json)与[成本](../../suba/docs/workflows/k0/I-cost.json)单列，原发布证据不覆盖。完整边界与复现见 [Q 记录](next/q/README.md)。

维护中的验收工具会在优化模式下提前拒绝运行。冻结种子工具 `verify.py`、`check_reference.py` 必须用普通Python，不能设置 `PYTHONOPTIMIZE` 或使用 `-O/-OO`；其原字节与种子身份保持不变。

原 `app/`、`compiler/` 只作为早期输入处理夹具保留于 `tests/fixtures/b2-input/`，不参与 Q 自编译。旧 0.5、R1a 和 B3.1 的文档不自动成为本版功能承诺。
