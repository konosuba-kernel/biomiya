# 2026-10-14 仓库目录迁移

本次按职责整理目录，保留原工作区 K0 改动，不改变编译器 .vt/.s0 内容、模块名、语言或 ABI。

| 原路径 | 当前路径 |
| --- | --- |
| core-s/next/q/ 的生产源码与 runtime/ | compiler/ |
| core-s/next/q/tests/ | tests/compiler/ |
| core-s/next/tests/ | tests/language/ |
| core-s/next/trials/ | tests/trials/ |
| core-s/next/system/ | experiments/system/ |
| core-s/next/ 顶层 P 源码、清单与 seed.json | bootstrap/transition/ |
| core-s/src、bootstrap、runtime、tests、fixtures、Cargo 文件 | bootstrap/frozen/core-s/ 下的同名路径 |
| core-s/tools/verify.py、check_reference.py | bootstrap/frozen/core-s/tools/，tools/ 中提供薄入口 |
| 其余 core-s/tools/ | tools/ |
| core-s/docs/reference/s-next-* | docs/reference/ |
| core-s/docs/reference/b3.1.* | bootstrap/frozen/core-s/docs/reference/ |
| 根目录旧 src、tests、tools、runtime、platforms、examples、vendor、Cargo 文件 | legacy/v0.5/ 下的同名路径 |
| 根目录旧 docs/ 顶层文件 | legacy/v0.5/docs/ |
| docs/workflows、docs/archive、docs/reviews | docs/history/ 下的同名目录 |
| core-s/next/ 阶段文档、评审、历史元数据 | docs/history/s-next/ |
| core-s/next/q/ 发布、维护、成本记录 | docs/history/s-next/compiler/ |
| core-s/build/ | build/ |
| 根目录旧 build/、target/ | legacy/v0.5/build/、target/ |

seed.json 原字节不变，71 个 source_sha256 路径相对于 bootstrap/frozen/。共享的四个 Rust 文件和基线测试日志保留冻结副本。原哈希、原历史证据不批量改写。

维护清单统一从仓库根目录解析；冻结验证器在保留原布局的内部树执行。旧 P3 记录继续保留，但新目录必须生成新 P3/P4 证据。当前成本统计写入 build/cost.json，不覆盖历史成本。

相邻 suba 的 ABI 工具和当前操作文档同步使用新路径，历史 K0 记录保留原内容。迁移检查运行 python3 tools/check_layout.py；固定点与专项结果写入新的 build/layout-*，汇总见 [迁移验证记录](layout-verification.json)。
