# Blocking：一个只读扩展分析

该试点回答一个有限问题：指定函数的直接调用闭包，是否会到达被外部合同标记为 `may_block` 的操作？它读取 Core 已完成的 IR，不改变语言规则或生成代码。`Verified` 仅表示这个属性在列出的外部假设下成立；不证明终止性、实时性、锁正确性、IRQ 安全或并发安全。忙等、递归和循环不会凭其形状被识别为阻塞；真实 `.s0` 实现也不会被自动推断为安全。

```sh
qemu-riscv64 -s 1048576 build/c2.elf check app.build --blocking app.blocking
qemu-riscv64 -s 1048576 build/c2.elf build app.build --blocking app.blocking
```

仓库中的 `pure.blocking` 用这项分析约束编译器自身的六个整数/布局规则入口，可直接与 `bootstrap/c2.build` 等自编译清单组合使用。它检查调用闭包，不代替源码上的纯函数审查。

关闭时仍使用原来的 `build MANIFEST` / `check MANIFEST`。全部要求通过时，开启与关闭分析生成的完整 ELF 字节相同。Rust C0 负责启动目标编译器，未增加此分析命令。

配置示例：

```text
format 1
require app::run
nonblocking platform::poll
may_block platform::wait
```

- 配置路径相对启动目录。最多 16 KiB，空行和以 `#` 开始的整行注释允许；每个有效行恰有两个词，LF/CRLF 均可。内联注释不支持。
- `format 1` 必须先出现且只有一次，至少有一个 `require`。声明名字是源码中的完整模块名，可带一个 `root::` 前缀；不使用 import 局部别名。
- `require` 只接受本次构建的普通函数，包括模块私有函数。不存在、重复或非函数根均拒绝。
- `nonblocking` / `may_block` 只接受 extern 声明。同一 linker symbol 的多个声明共用一份合同；重复或冲突合同均拒绝。未知名字、内部函数合同也拒绝。
- 未提供合同的 extern 为 `Unknown`，即使本次构建提供了它的 `.s0` 实现。报告列出显式假设；合同可信性由提供者负责。

结果逐根报告：

| 结果 | 含义 | 严格构建 |
| --- | --- | --- |
| `Verified` | 可达外部调用均声明为 nonblocking，或没有外部调用 | 所有根都如此才允许发布 |
| `Violation` | 可达至少一个 may_block 外部调用 | 拒绝 |
| `Unknown` | 无已知违反，但可达缺失合同的外部调用或未捕获函数 | 拒绝 |

同时可达已知阻塞和未知调用时报告 `Violation`。未调用的外部函数不会污染要求根；IR 中保留的不可达调用仍保守纳入。`defer` 展开产生的 CALL 同样纳入。现有语言只支持直接函数调用；未来若增加间接调用，必须先扩展 Unknown 规则，不能自动把它视为通过。

配置在声明完成后加载；每个函数 IR 构造完成、arena 重用前提取 DeclId 调用摘要。后端继续验证并生成代码，所有链接检查完成后统一判定，最后才可能新建输出。`check` 永远不发布 ELF；分析失败不留下新输出。已有输出通过既有独占创建机制保护。

实现保留最多 8192 个声明和 65536 条 CALL 摘要，不保存临时 AST/SlotId。每条反向边保存调用者和链表索引；每个函数最多获得 may-block、unknown 两个效果位，因此工作队列最多入队 16384 次，传播为 O(V+E)。递归环无需递归遍历或全程序 AST。边数量超限报告 `I_LIMIT`；配置错误报告 `E_BLOCKING_CONFIG` 和配置字节位置。

这是具体的一项分析，不提供插件 ABI、通用属性求解器或可修改 IR 的扩展接口。
