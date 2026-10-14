# 迁移映射与成本口径独立审查

2026-10-13，`/root/resume_review`。只读审查，不修改映射、成本工具或正式测试。

**映射与成本说明无阻断问题；这不是最终候选功能接受。**

审查身份：`next/migration-coverage.json` SHA-256 `16e743a6edd79b078b8ab805526718e2f2a9143c925c0bf4441d279d211ef04d`；`next/q/cost.md` SHA-256 `7a7a9098c09d21ad2cf8d720da7e5e824256582b71464ac0695570675fca3785`。

- 91 个唯一测试名与 `seed.tests` 精确相等，每项有具体 suite/case 选择器。未把名称映射或测试数量当作执行通过。
- 旧 Rust renderer/第二编译出口和 --root 删除已明确；对应运行/机器职责由具体探针承担。前向 const、类型名字空间、固定 Result/?、defer 和文本容量变化有解释，没有要求恢复旧内部接口。
- 对照旧常量 walker 的 14 个表达式，新 interfaces 的合法值和非法 const/static 分支逐项相符；limits 的 15 项包含原缺失的巨大数组与四类长链；纯规则/机器职责独立列入相应 suite。
- 复制应用与故障注入说明区分“先真实 close 再模拟错误返回”和真实内核 close 失败状态，没有夸大证明。Blocking 内部和 copy 场景尚需最终运行，顶层 status 明确未验。
- 当前开发记录：interfaces-r4 76 项与 blocking-r2-external 44 项绑定 R-ID r2；limits-r2 15 项绑定更早 r1。不可将不同候选的通过直接拼成最终集成接受。
- 从 P、Q r1 和 R-ID 源胶囊重新统计，cost.md 历史总数、P→Q 逐模块差分和 R-ID 增量一致。生产、支持、测试、宿主工具和冻结种子分账，没有隐藏主线 Blocking 或将测试生成器计为零成本。8,000 行目标未达的结论保留。

非阻断口径说明：`measure_next.py` 的非空非注释计算使用 VT/C 词法，除了 Python 的 `#` 注释，S0/.S 的 `#` 注释也继续计数；应在最终 JSON notes 中一起说明。物理行和源字节主账不受影响。最终 cost.json 需在生产与工具冻结后重新生成；本次不将旧 r1 cost.json 当作当前生产账。

最终关闭条件仍为：新候选全部 suite 执行成功、选择器对应实际通过记录、输入和编译器哈希一致、独立整合判定通过。
