# biomiya 代码规约审计

2026-10-14。用户要求：用 suba/CODE_STYLE.md 0.1 检查 biomiya；本轮只审计和记录，不重构生产代码。
基线：`2c7061cbed13959697369879e5717a8ad0c9fb21`，初始工作树干净。

| 节点 | 输入/依赖 | 输出及唯一写入范围 | 作者 | 独立核验 | 验收条件 | 状态 |
|---|---|---|---|---|---|---|
| A | 规约、当前Q前端 | `frontend.md`，`frontend-*` | `/root/biomiya_style_frontend` | `/root` | 具体代码位置、违反的规则、影响/反例、可行改法；长函数仅是信号 | 已独立接受，见acceptance-ab.md |
| B | 规约、Q所有者/后端/runtime | `boundaries.md`，`boundaries-*` | `/root/biomiya_style_boundaries` | `/root` | 区分真实bug与维护风险；检查状态、错误、unsafe、清理、目标依赖 | 已独立接受，见acceptance-ab.md |
| C | A/B通过；root目录/工具调查 | `REPORT.md`、`metrics.json`及审计辅助证据 | `/root` | `/root/biomiya_audit_acceptance` | 覆盖范围诚实、统计可复查、证据和严重性成立、优先级可执行 | r1独立接受 |
| D | C冻结候选 | `acceptance-*.md` | `/root/biomiya_audit_acceptance` | 作为C及组合的独立验收 | 不将统计或历史格式当bug；无未经核实的主要结论 | Accepted，见acceptance-r1.md |

DAG：A/B并行→C→D，无环。各作者写入范围不重叠；root可提前做只读盘点/工具调查。当前Q和维护中的工具重点审查；冻结B/P/Rust/旧0.5/实验先盘点和检查依赖边界，不能冒充逐行重审全部历史代码。每份报告标明实际覆盖和未覆盖。

规则为审计标尺，不把新规约追溯成旧版本正确性失效。候选冻结以文件哈希记录；旧判定保留。生产源码、旧release/证据不修改；验证可在临时目录运行。

最终：A/B由root独立接受；C-r1由 `/root/biomiya_audit_acceptance` 独立接受，无阻断修订。验收报告SHA-256：`b1e7da5ba1280f2268a97bc962aba4dd50cfe5067d20886f919ebd04f2cedfdc`。生产tracked diff为空，仅本审计目录新增报告与文本证据；未提交。
