# biomiya

konOSuba 的教学系统语言与自举编译器。当前维护实现为 **S-next 0.1**，优先级是自举、简单、现代。编译器用 S-next 编写，在 RV64 Linux-user 下运行，支持固定的 linux-user、suba-user、suba-boot、suba-kernel 输出 profile。Linux-user 自编译不等于 suba 内自举。

从 [语言参考](docs/reference/s-next-0.1.md)、[构建与验证](docs/building.md) 和 [VS Code 插件](editors/vscode/README.md) 开始。所有维护命令统一从仓库根目录执行。

| 目录 | 职责 |
| --- | --- |
| [compiler/](compiler/README.md) | 当前编译器、目标后端和运行支持 |
| [bootstrap/](bootstrap/README.md) | 冻结的 Rust/B3.1 种子及过渡 P |
| [tests/](tests/README.md) | 当前语言、内部合同与表示实验的回归输入 |
| [tools/](tools/README.md) | 引导、自编译、专项回归和目录一致性检查 |
| [editors/](editors/) | 独立编辑器插件 |
| [examples/](examples/) | 当前可运行示例 |
| [experiments/](experiments/) | 系统切片及其他实验 |
| [docs/reference/](docs/reference/README.md) | 当前语言参考和版本索引 |
| [docs/design/](docs/design/) | 设计方案，不直接代表功能承诺 |
| [docs/history/](docs/history/README.md) | 保持原内容的历史交付、验收和身份记录 |
| [legacy/](legacy/README.md) | 旧 0.5 实现 |
| build/ | 忽略的本地构建和验证产物 |

原有未提交的 K0 工作随目录迁移保留，本轮不改变编译器语义。旧路径映射与冻结约束见 [目录迁移说明](docs/layout-migration.md)。已有平台结果见 [K0 实施记录](../suba/docs/workflows/k0/DELIVERY.md)，源码规模、功能边界和自举正确性分别报告。
