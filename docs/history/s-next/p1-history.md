> 历史 P1 阶段记录。这里的支持范围、路径和构建命令按该阶段描述；当前入口见 [README](README.md)。

# S-next P1 执行语义试点

2026-10-12。已执行[下一版方案](../../docs/design/core-s-next.md)的 P0 和 P1；P2–P5 尚未完成。这里是用 B3.1 编写的新编译器 P，接受 P1 的过渡输入语言。它还不是完整 S-next，也没有新语言自编译固定点。

## 目录与引导边界

- 本目录的 13 个 `.vt` 模块是 P 的源码，`compiler.build` 显式选择它们。内部模块名暂沿用 `bootstrap::`，模块并不按目录自动搜索；不要把本目录和旧 `bootstrap/` 同时放进一个清单。
- 旧 `core-s/bootstrap/`、Rust 种子、运行库及其测试保持 B3.1 基线。P 当前复用旧运行支持作为其实现依赖，因此可以在实现源码中使用旧 Result 和问号；它由旧编译器 B 编译。
- [seed.json](seed.json) 固定 71 个引导依赖文件的哈希、目标 B 的哈希及 91 个旧测试身份。验证器发现这些文件变化即拒绝继续，不依赖未提交工作区的 HEAD 标签识别版本。
- [test-migration.json](test-migration.json) 为全部 91 项旧测试记录后续迁移意图。它不代表 S-next 已通过同样的 91 项测试。
- [tests/cases.json](tests/cases.json) 和对应 `.vt`、`.s0` 是本阶段的 37 个行为与拒绝样本；[check_next.py](../tools/check_next.py) 执行验收。

新能力不回填 Rust。P4 将源码迁入新语言成为 Q，再完成 E0→E1→E2→E3；当前不能用 P 编译 P 的旧语言源码来要求或宣称自举。

## 当前接受集的变化

| 项目 | P1 的实际行为 |
| --- | --- |
| 赋值 | 先计算并检查目标地址，再求右值，最后存储；右侧不重新决定目标 |
| defer | `defer cleanup(args);`，只接受声明过的直接函数/extern 调用，结果必须为 unit |
| 参数捕获 | 注册时从左到右求值并保存值；聚合复制自身，指针复制地址 |
| 退出 | 块正常结束、return、break、continue 按所退出作用域逆序清理；返回值先保存 |
| panic | 不展开；清理函数 panic 后不继续执行其他清理 |
| 权限 | 延迟调用 unsafe/extern 时，注册位置必须处于 unsafe；不在清理执行处重新判断权限 |
| 局部初始化 | let 和 var 都必须初始化 |
| 删除的输入形式 | 块式 defer 与 `?` 报 `I_UNSUPPORTED` |
| 临时保留 | 旧 Result 构造和 match、旧常量/布局机制、显式局部类型及旧结构构造语法 |

尚未实现一般变体、zero、make 构造、单向局部定型、独立声明语法和新的模块规则；不得把完整方案当成本阶段的语言参考。

## 构建与验证

以下在 `core-s/` 执行。推荐使用同一验收命令完成构建和测试：

```sh
python3 tools/check_next.py --seed build/b31-refactored.elf --record-dir build/snext-check
```

若本地没有冻结种子，可从保留的源树重建：

```sh
python3 tools/verify.py --record-dir build/rebuilt-b31
python3 tools/check_next.py --seed build/rebuilt-b31/compiler.elf --record-dir build/snext-check
```

证据目录必须是新目录。验证器核对种子 ELF 和全部冻结输入，在临时目录只复制声明输入，生成 `compiler.elf`，再运行样本和旧目标端测试。构建跟踪检查输入白名单与无宿主代编译；目标 PATH 为空、guest 栈为 1 MiB。记录内包含可重建种子的确定性 `seed-source.tar`、候选源码和验证输入哈希、命令输出及 ELF。

只构建 P 可使用：

```sh
qemu-riscv64 -s 1048576 build/b31-refactored.elf build next/compiler.build
```

这会独占新建 `build/snext-p1.elf`；存在时拒绝覆盖。候选的 CLI 仍是 `build|check MANIFEST [--blocking POLICY]`。P 的编译统计前缀为 `S-next-P1`；用 B 构建 P 时看到的 `B3 ... max_saved=...` 是旧编译器 B 的统计，不能当作 P 仍含捕获机制的证据。

## P0 与 P1 验收

P0 在干净源树重建，91 项旧回归通过，C2/C3 相等，得到的 B 与冻结哈希一致。源码集合身份为 `cd6336297d22dd27efe29cbcc4fe0f0b88c7f4882e9f81fdce6e2f2e5415397d`。本地证据为 `build/snext-p0/verification.json`；源文件逐项身份在 seed.json 中保存。

P1 的最终本地证据为 `build/snext-p1-verified-r3/verification.json`：

- 37 个新样本通过 check/build/执行或拒绝检查，包括实际 Linux 文件读取失败后关闭、1,024/1,025 个活动 defer 的容量边界、2,048 次循环清理、参数与返回快照以及 panic 路径。
- 22 个旧手册样本全部运行，按新规则显式改变 4 个期望：赋值结果从 12 改为 21；三个块式清理样本改为拒绝。其他期望保留。普通 FFI 与延迟 extern 都验证 Blocking 三态和分析开关同 ELF。
- 11 项原有目标端输入/机器测试对新 VT 实现运行通过，覆盖词法/输入、机器编码、非法 IR、ABI、数据与链接等合同。测试驱动由冻结 Rust 编译，但被测的是覆盖进临时树的新 VT 模块。

因此本阶段是 **59 个语言/行为样本 + 11 项沿用目标端测试**，与 P0 的 91 项冻结基线回归分别计数。首次验收脚本漏设临时 seed ELF 的可执行权限，已修复；失败记录 `build/snext-p1-verified/` 保留，不作为通过证据。

P1 目标 ELF SHA-256：

```text
64e8b6be297799b55daf7d8b22098c61e49ea66b81a2ff910de6870aa9378cf1
```

本地通过验收的编译器是 `build/snext-p1-verified-r3/compiler.elf`，与 `build/snext-p1.elf` 相同。产物和运行日志属于 Git 忽略的本地 build 内容，验证命令可重新生成。

## 实际删减与保留责任

赋值直接发射目标地址、右值与 STORE。普通调用与 defer 共用 `prepare_call`，在嵌套实参求值完成后保存连续的实参范围。CleanupCall 只保留被调用声明、参数范围和源位置；各出口发射普通 CALL。

已删除 `Mark`、`Fragment`、`begin_capture`、`end_capture`、`instantiate`、65,537 项指令备份表、`in_defer` 状态以及问号展开。跨函数调用摘要的 `blocking.capture` 是另一项功能，仍保留。

| 指标 | 冻结 B3.1 | P1 |
| --- | ---: | ---: |
| 生产 VT 物理行 | 7,988 | 7,886 |
| 生产 VT 源字节 | 240,327 | 237,346 |
| ir.vt 物理行 | 315 | 214 |
| 目标 ELF 字节 | 1,079,368 | 1,067,080 |
| 目标 RW 段字节 | 53,118,748 | 49,981,148 |
| 目标映像跨度字节 | 53,995,292 | 50,845,404 |

物理行包含空行和注释；运行库、冻结引导源、测试及工具另列。RW 减少 3,137,600 字节，约 3 MiB，主要来自移除捕获存储。这不是进程峰值 RSS，也不是新语言自编译内存测量。函数作用域、值快照、聚合复制、清理调用展开、原始访问和机器验证仍是必要责任。

下一阶段是 P2：解析声明头一次，分离名字绑定和布局，切断布局前常量对布局查询的依赖，再删除 token 求值器和扫描器保存/恢复路径。
