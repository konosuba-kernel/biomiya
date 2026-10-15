# 构建与验证工具

从仓库根目录调用。证据写入新的 --record-dir；完整顺序见 [构建说明](../docs/building.md)。

| 工具 | 用途 |
| --- | --- |
| verify.py | 冻结 B 重建的受保护入口 |
| check_next.py | 用 B 构建并验证过渡 P，完整引导使用 --stage P3 |
| selfhost_next.py | B(P)、P(Q)、多代固定点与源码扰动 |
| check_q.py | 当前语言回归 |
| check_q_machine.py | AST/IR/机器码合同 |
| check_q_input.py、check_q_copy.py、check_q_io_failures.py | 输入与 I/O |
| check_q_diagnostics.py、check_q_interfaces.py、check_q_limits.py | 诊断、接口、容量 |
| check_q_blocking.py | Blocking 分析 |
| check_q_host.py、check_q_profiles.py、check_q_s0_system.py | 宿主桥、目标、汇编 |
| check_next_reference.py、check_reference.py | 当前与冻结参考示例 |
| check_q_tool_guards.py | 优化模式拒绝及错误预期负例 |
| measure_next.py | 成本统计，写入 build/cost.json |
| check_layout.py | 冻结身份、清单路径、活动文档链接 |

脚本名暂保留 next/q，方便对照历史报告。冻结工具在 bootstrap/frozen/core-s/tools/ 中原样保存，顶层 verify.py 和 check_reference.py 仅作带优化模式保护的入口。
