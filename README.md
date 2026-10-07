# biomiya

面向裸机与操作系统内核的语言设计。当前参考版本为 0.5。Core 负责语法和编译执行，Extend 按需增加正确性检查与分析。

编译器实现基线：

- [语言参考手册](docs/reference.md)：Core 规则、完整 EBNF、内建接口、诊断类别及扩展边界。
- [RV64 目标补充](docs/architecture-rv64.md)：rv64-core-v1 的布局、调用约定、运行时、陷阱与汇编接口。
- [合规范基线](docs/conformance.md)：接受、拒绝、运行与扩展用例的预期结果及平台验收要求。
- [机器可读用例索引](tests/conformance/manifest.json)。

编译器交付设计：

- [Core S 最小语义方案](docs/design/core-s-semantics.md)：以显式类型、统一值复制和有限内核接口收束 Core 的下一版设计；尚未实施，不替代当前 0.5 合规范声明。
- [Core 完整交付需求](docs/core-delivery.md)：已确认决策、交付范围、验收层次与实施里程碑。
- [Rust 编译器架构](docs/compiler-design.md)：前端与语义 IR、RV64 后端、平台运行支持及 Extend 接口。
- [实施 workflow 与验收门槛](docs/workflow.md)：并行所有权、集成循环与完整交付 Gate。

仓库提供 Rust 编译器与参考运行平台，支持检查、RV64 汇编／对象／ELF 构建和 QEMU 验收。[构建与运行](docs/building.md)说明使用方式、实现边界和完整验收入口。完整交付由条款审计及 `build/acceptance.json` 的实测结果确认；Extend 仅交付接口，真实验证器未提供。[draft](docs/draft.md) 仅保留设计历史，不作为实现依据。

0.5 修订了泛型布局判定，并补齐原始存储／切片接口、外部静态符号、Copy 数组重复构造及原子顺序规则；范围与暂缓项见[修订说明](docs/reference.md#附录-i-05-修订范围与实现取舍)。rv64-core-v1 保留既有布局和 ABI，以及不在陷阱处理函数内调度的首版平台限制。
