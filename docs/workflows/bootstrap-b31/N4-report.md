# N4 r1 实施报告

输入 N3-r1，作者 `/root`。新增 `bootstrap/blocking.vt` 与可选 `--blocking POLICY`，配置和证明边界见 `core-s/bootstrap/blocking.md`；`pure.blocking` 提供编译器自身六个规则入口的实际配置。

分析在声明稳定后加载配置，当前函数完成 IR 后提取稳定 DeclId 调用摘要，在 arena 重用前保存。反向边和最多两次入队的双位传播支持递归，复杂度 O(V+E)。缺外部合同保持 Unknown，已知 may_block 优先报告 Violation。相同外部 linker symbol 的别名共享合同且拒绝重复/冲突。最多8192声明、65536条边、16KiB配置；没有通用求解器或源语言扩展。

Core 前端、机器和链接验证仍完成；最终 ELF 构造成功后统一判定全部要求，全部 Verified 才可能调用原有独占保存。分析不写语义表，不保存临时 SlotId/AST；关闭分析的路径不加载配置和调用摘要。

正式执行 b3 集成7项全部通过，包含原3项和新增4项。新测试覆盖三代报告/门禁/ELF逐字节相等、递归与defer、合同严格性和外部别名、300调用链顺序、失败文件保护、缺失capture、非法CALL和容量65537。原有C2/C3固定点及源码扰动继续通过。详细命令与范围在 N4-validation.json，原始日志 N4-b3-tests.log。

本次只改变 main、增加分析及配置/说明、更新manifest与b3 source列表/测试；N3纯规则和后端等其余产品保持冻结内容。原型/tmp文件仅用于实施准备，不作为正式通过证据或运行依赖。N5执行旧B3迁移、干净全量测试、实际self-policy和完整规模统计。尚待独立验收。
