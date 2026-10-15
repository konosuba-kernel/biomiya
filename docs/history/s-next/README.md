# S-next 执行记录与引导边界

当前维护源码是 [q/](q/)，语言使用规则见 [S-next 0.1 参考](../docs/reference/s-next-0.1.md)。`next/` 顶层的 14 个 VT 模块是一次性过渡编译器 P，由冻结的 B3.1 编译；P 接受新语言，但自身使用旧实现语言，不能要求 P 编译 P。

2026-10-14 K0的当前产物为 `build/k0-p4-r2/e3.elf`，SHA-256 `049c6e094ccd044fba3b5d357583013b77f0e8e9a98d431ad919e02b0cda2144`。同一E3的本轮回归及独立判定见[维护记录](q/k0-2026-10-14.json)；下方P0–P5记录保留上一轮架构交付身份。

## 引导链

```text
冻结 Rust → 冻结 B3.1（B）
B(P) = E0
E0(Q) = E1
E1(Q) = E2
E2(Q) = E3
```

`seed.json` 固定 B 的完整 ELF 和 71 个重建输入；当前未提交维护重构源码使用内容哈希识别，不能用旧 HEAD 冒充。P3 候选为 `build/snext-p3-r1/compiler.elf`，E0 SHA-256 `4e538e5fa918470752d90323c50bfdcfed4f7c8e0ee75cc0d75f39dc8700263d`。

P4 r2 已完成完整固定点和源码扰动：E1=E2=E3，SHA-256 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。工作区产物为 `build/snext-p4-r2/e3.elf`。稳定 ID 与所有权架构返工已独立通过。八套最终 E3 检查及 P5 的判定见 [交付 DAG](delivery.md) 与 [Q 记录](q/README.md)。产物不随 Git 分发，可按 core-s 构建入口重建。

## 分阶段证据

| 阶段 | 实现与验收 |
| --- | --- |
| P0/P1 | 冻结 B、赋值顺序、参数快照和保存调用式 defer；[历史记录](p1-history.md)、[独立审查](reviews/p1.md) |
| P2 | 独立声明语法、绑定/常量/布局、InitData、make/zero/单向定型；r5 的 98+11 通过，r4 指针数组递归缺陷已修；[审查](reviews/p2.md) |
| P3 | 具体枚举支持、私有语义表、生命周期与数据试点；126+11、另 8 个试点通过；Q 实际 AST/IR、NameId/TypeId 与存储拥有者已独立验收；[审查](reviews/p3.md) |
| P4 | Q r2 完整固定点与源码扰动；八套最终 E3 检查和独立判定见 [P4 r2](reviews/p4-r2.md) |
| P5 | 真实文件回滚、Sv39 页表、16550 UART、双 hart 原子协议；初版 PTE 高位截断已修；使用最终 E3 的 r3 判定见 [审查](reviews/p5-r3.md) |

原 91 个测试的迁移意图保存在 [test-migration.json](test-migration.json)；[91 项实际映射](migration-coverage.json)与[版本记录](q/release.json)绑定同一最终 E3 的具体场景；实际保留/改变/删除能力以这些证据为准，不以旧源码兼容率作为指标。探索性 trials/system 只有绑定到被验收编译器的实际运行证据才计入阶段。

## 规模结论

2026-10-13架构基线Q生产 10,991 行、325,234 字节，成本另列于 [基线账本](q/cost.json)。2026-10-14行为修复后当前Q为11,018行、326,623字节，VT runtime另290行；[本轮账本](../../docs/workflows/behavior-fixes-2026-10-14/I-cost.json)分别统计生产、运行库、工具与新测试。8,000 行暂定预算未达，不宣称净缩减成功。当前目标 RW 55,135,764 字节、映像跨度 56,143,380 字节，固定点验证的 guest 栈为 1 MiB；静态预留不是峰值 RSS，单帧统计不是调用链峰值。

语言或 Q 的生产实现后续只改 Q。冻结的 Rust/B 和 P 不长期同步演进，也不把 Linux-user 自编译称作 suba 内自举。
