# A/B独立验收

审查者 `/root`，2026-10-14。未参与frontend/boundaries作者报告写作。

接受 A：核对check::struct_value/init::build_struct及types::locate/fail的时序；四例check/build独立复跑8次，与作者结果一致，见frontend-root-verification.json。核对切片公开接口、InitData重载字段、38变体Syntax及窄投影。A1是诊断缺陷，A2合同债务，A3维护风险，A4可选结构改进；不得将后三者伪装为已复现内存/错码问题。

接受 B：阅读runtime路径复制、input::save和注入shim，再复跑两个探针，分别记录boundaries-root-remove.json和boundaries-root-cleanup.json。确认NUL前缀删除与双故障残留；明确manifest已排除NUL、unlink失败不可能承诺强制回滚。核对公开状态写入点、model/input直接syscall、rv64::start和image布局。start在主循环前发射，没有“发射到函数之后”的错误结论。

两节点的owner/epoch/seal正面结论与代码相符；不接受用新内核IRQ规约否定用户态编译器递归/阻塞。作者候选文件保持冻结，最终解释与优先级由REPORT整合。
