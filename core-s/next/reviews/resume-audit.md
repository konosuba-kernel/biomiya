# 2026-10-13 恢复审计

独立验收 agent：`/root/resume_review`，未参与生产实现或验收工具编写。此次先核对旧判定与新增证据，不接受尚未冻结的生产文件。

- P4 r1 的固定点与架构拒绝结论继续有效，不能用前者抵销后者。
- `build/q-blocking-r1-external/verification.json` 只证明旧 r1 编译器上的外部策略检查；`external_only=true`、`internal_contracts_passed=false`。missing capture、bad IR、65,537 summary edge 必须由最终候选运行真实 Q analysis 探针。
- 对照旧 `tests/b3.rs` 的四个 Blocking 测试，新工具最初缺纯递归闭环的 Verified 分支。实施者已认领补齐；未冻结的新工具不在此接受。
- `build/q-limits-r1` 的 10 项不能覆盖原 frontend 容量矩阵全部断言。缺 3,000 级 field/index/else/静态算式链与巨大结构数组乘法溢出；原 type-shadow 测试的规则变化需列入迁移说明。
- 使用冻结 r1 E3（SHA-256 `57b3bb82648ea7e9572b2547f7ef25291b0d3ea727264aa36de4671c47ac7d3d`），独立执行上述五个资源反例，均退出 1、报告 `I_LIMIT`、无 ELF。源、命令和结果见 `build/resume-independent-gap-probes-r1/results.json`，1 MiB guest 栈、空 PATH。这些结果定位为覆盖缺口，没有发现新崩溃；不能代替最终修订重新执行。
- `migration-gaps.md` 中 G3–G13 仍须按保留职责逐条收口，不能由语言/机器样本总数直接推出。主线正在补接口、复制及故障注入工具，本审计不重复实施这些任务。
- 8,000 行软预算尚未达到；最终须量测完整生产与支持成本。资源硬门槛与可维护性合同各自判断。

接续门槛：R-ID 冻结及独立通过 → R-Owner 冻结及独立通过 → 新候选固定点、全部专项和 P5 → Integration 独立验收。此文不改变 `p4.md` 的总体不接受判定。
