# 四个行为问题修复

2026-10-14；基线2c7061cbed13959697369879e5717a8ad0c9fb21。输入为已独立接受的 docs/reviews/style-2026-10-14/REPORT.md 和 suba/CODE_STYLE.md 0.1。沿用dag-delivery。开始时仅审计目录未跟踪。

范围：F1优化模式假通过、F2 NUL删除、F3保存失败残留信息、F4聚合字段诊断。K0平台/接口收紧后续实施，不重写AST/InitData，不改冻结B/P/Rust/旧证据，不自动提交。

| 节点 | 依赖 | 写入范围 | 实施者 | 独立验收者 | 必需验收 | 状态 |
|---|---|---|---|---|---|---|
| T | 已接受审计 | core-s/tools既有验证脚本及新工具自测；本目录T-* | /root/fix_verification_tools | /root | 优化模式不能假通过，普通流程不退化；输入hash/拒绝边界不放宽 | T-r1已独立接受，见T-acceptance-r1.md |
| R | 已接受审计 | next/q/runtime/linux.vt、input.vt；q/tests文件名behavior-io-*；新tools/check_q_io_failures.py；本目录R-* | /root/fix_io_failures | /root | NUL开删一致拒绝；正常保存/已有输出保护；短写、写/chmod/close/清理故障区分，主错误+残留可见，无不必要chmod | R-r1已独立接受，见R-acceptance-r1.md |
| D | 已接受审计 | next/q/check.vt、init.vt及parse.vt::make_expression字段起点；新tools/check_q_diagnostics.py及q/tests/behavior-diagnostic-*；本目录D-* | /root/fix_diagnostic_spans | /root | 局部/static未知/重复/私有/遗漏字段位置；有效构造不退化；不把父span永久覆盖子IR位置 | D-r2已独立接受，见D-acceptance-r2.md |
| I | T/R/D最新候选独立接受 | 新build/behavior-fixes-*证据；文档、独立新发布/成本记录；本目录I-* | /root | 固定点、实际源码扰动、四问题回归和受影响消费者；当前源码/工具hash绑定 | I-r1已由A独立接受，435项及自举/扰动/P5/工具检查通过 |
| A | I冻结候选 | 本目录acceptance-* | /root/behavior_fixes_acceptance | 该代理对整体作独立判定 | 核对真实产物/固定点/反例/源码身份，全部必需项通过 | Accepted，见acceptance-r1.md；验收者 /root/behavior_fixes_acceptance |

DAG：T/R/D并行→I→A。图无环，实施写入互斥；共享tools中R/D各自仅写其专属新文件，T不碰它们。节点候选按hash冻结，root不修改其代码而独立验收；返工交回作者并递增版本。I不覆写旧发布证据；新证据与旧基线区分。最终组合由未参与实施者审查。

T冻结边界澄清：seed.json明确把 tools/verify.py、check_reference.py 列为冻结种子输入。二者保持原字节；维护工具须在导入它们前拒绝优化模式，历史入口自身限制在交付中明确，不重写旧种子身份。

D-r1实测发现FieldValue本身沿用整条make的起点，显式传递已有span不足以修复。D范围扩展至parse.vt::make_expression，只记录每个字段名真正起点；保留失败r1，转D-r2。此为原F4必要根因修复，不扩展语言承诺。

I启动记录：首次批命令cwd误设core-s，导致D验收文件写入/IR复跑路径失败，但P3预备构建已启动。随即在正确根目录复跑IR exit0、落盘D-r2验收，之后才计入I正式验收；没有把失败路径调用记为通过。

在I自举运行期间，最终审查者仅提前只读检查已接受T/R/D的组合源码，属于探索，不构成A启动/通过证据。A正式验收必须等待I冻结。

最终收尾：A 对 I-r1 判定 Accepted。候选清单 SHA-256 e0acbc4679dd83464fb62ebac5d9d113df506df94fc431542b77481abdd5133d；独立验收记录 SHA-256 17723db20b21f1b4fad434cc5e1c7d0014f17816c559e7107138c7498fecd06e。验收后再次核对58个候选文件无变化；冻结种子、历史发布与成本记录无diff。独立复跑 IO12、诊断18、工具88次入口检查及5次语料调用通过；5次语料调用中2次实际编译运行、3次优化模式早拒绝。所有必需节点已接受；K0后续，当前修改未提交。
