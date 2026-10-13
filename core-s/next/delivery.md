# S-next 全计划执行 DAG

2026-10-12。用户要求执行所有计划。生产文件仅改 next/，冻结基线不动。

依赖：P1-review → P2 → P3 → P4 → P5 → Integration。P5 样本可提前探索，不计验收。

| 节点 | 合同与范围 | 实施者 | 独立验收者 | 状态 |
| --- | --- | --- | --- | --- |
| P1-review | 核对 r3 源哈希与 59+11 证据，补充区分性探针；只读 next/ | 既有 root | /root/next_review | 已验收 r3（报告 reviews/p1.md） |
| P2 | 方案第3.5/3.6与第4节：声明语法、无扫描器布局、统一表达式、InitData；next/ | /root/next_frontend + /root | /root/next_review | 已验收 r5，98+11；指针数组递归、绑定阶段边界已修复（reviews/p2.md） |
| P3 | 方案第3.4/3.7/5节：具体变体、ID/状态边界、定型与成本 | root + next_frontend + next_system | next_review | P 表示支持/试点通过 r1；Q 名义 ID 和实际拥有者已验收（reviews/r-id.md、reviews/r-owner.md） |
| P4 | Q源码、三代固定点、源码扰动、目标机器验证、新版本手册 | root + next_frontend + next_system | next_review | r2 已独立通过：三代固定点/扰动及405项检查，见 reviews/p4-r2.md |
| P5 | 位图/页表、真实文件回滚、设备/原子和完整程序 | /root/next_system | next_frontend / next_review | 最终E3的r3两程序及10负例已独立通过（reviews/p5-r3.md） |
| Integration | 当前组合重新构建、全部规则测试、完整账本及独立判定 | root | /root/resume_review（系统另由resume_ids） | 已独立通过；554项组合审计，见 reviews/integration.md |

所有节点依照 docs/design/core-s-next.md 验收。证据保留旧版本，内容哈希标识候选；上游变化使依赖验收待复核。实现完成不等于独立通过。

当前 Q 的生产规模为 10,991 物理行，8,000 行原型软预算未达标；64 MiB 映像及 1 MiB guest 栈仍为硬门槛。不得把功能验收等同于复杂度目标达成。

## P4 架构返工 r2

依赖顺序：`Q-r1 实测证据 → R-ID → R-Owner → R-Bootstrap → R-System → Integration`。

| 子节点 | 实施与写入范围 | 独立验收 | 合同 |
| --- | --- | --- | --- |
| R-ID | next_frontend；独占 Q 生产 VT | next_review | NameId/TypeId 实际用于生产字段与接口，私有 raw；Type 元素与声明身份不混用；合并重复语法解包，不以压行降低账本 |
| R-Owner | next_system；R-ID 冻结后独占 Q 及内部探针 | next_review | source bytes 完成后只读，模块元数据一次完成；映像数组私有、完成符号不能覆盖、追加/修补/封口明确；不得用万能 setter 隐藏原有跨模块写入 |
| R-Bootstrap | root；证据和版本文档 | next_review | 从 B/P 重新构建新 Q 三代固定点，全部语言/内部/参考测试及源码扰动，记录新完整输入身份 |
| R-System | next_system；冻结平台重测 | next_frontend | 使用新的 E3 重测真实平台与失败路径，旧 r2 平台证明不可冒充新编译器证据 |

R-ID、R-Owner 是原方案硬合同的补齐，不是新增功能；原 r1 自举事实保留，但不能据此宣布 P3/P4 全部完成。R-Owner 在 R-ID 独立通过前仅可做只读准备。

## 2026-10-13 恢复

前轮子代理因使用额度中断；工作区变更和旧证据保留。恢复后的实际角色：R-ID 实施 `/root/resume_ids`，R-Owner/机器测试实施 `/root/resume_owner`，独立验收 `/root/resume_review`，集成和其余迁移测试由 `/root` 实施。

R-ID 候选 `build/q-stableids-r2/compiler.elf` 已独立验收通过（`reviews/r-id.md`），SHA-256 `1e4b4c15f3bd613652901364e3ec8a2e5b0e04a5cbe278f3a1948e8f8e0942b0`；14 个生产 VT 为 10,906 行。R-Owner 已解锁。新候选尚不能继承 r1 的固定点、机器及系统验收。

原 91 项迁移的逐项审计见 `reviews/migration-gaps.md`。追加验证是对原合同缺口的补齐，不新增产品功能：Blocking、深度/布局、清单/诊断/发布、实际多文件复制与 I/O 故障、ABI/ELF/纯规则边界。相应新工具将在最终 Q 上统一执行，不能把开发阶段 r1 的通过当作最终证据。

R-Owner r1 已独立通过（`reviews/r-owner.md`），候选 SHA-256 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`，18 输入身份 `60b44ea782a1268f30d37bf6969fc4d53cf4dbb852cbf98cee3fbc87dbeb8d65`。当前 Q 为 10,991 行、325,234 字节。正式 R-Bootstrap r2 已解锁并执行，目录 `build/snext-p4-r2`；不继承旧 r1 下游验收。

R-Bootstrap r2 机械链已完成：E1=E2=E3 为 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`，源码扰动实际改变行为。最终 E3 的语言126、输入24、Blocking48（含1构建）、容量15、接口76、复制/I/O8、参考3通过；补测工具已独立通过（reviews/migration-tools.md修订三）。机器首轮103/105通过，两个AST负例导入列表增长后的file序号断言待修，记录保留；无生产源码变更。

机器 r2 仅同步两条精确诊断的 file=11（错误类别与 byte 不变），105/105通过、changed_inputs=[]，记录 `build/snext-final-machine-r2/verification.json`。原 r1 失败记录完整保留。最终八套套件合计405项（含Blocking的一条构建检查），等待R-Bootstrap独立总判定。

R-Bootstrap r2 已独立通过（`reviews/p4-r2.md`）。随后正式运行的 R-System r3 两程序及10平台负例通过，源码/编译器未变，等独立签收；`q/release.json` 已绑定全部实际证据与 `migration-coverage.json` 的91项，通过状态仍等待最后整合验收。

## 最终交付状态（2026-10-13）

P0–P5 的必需实现和独立验收已闭合。R-ID、R-Owner、R-Bootstrap r2、R-System r3 与最终 Integration 均通过；旧候选及失败记录保留，不改写为成功。

最终 E3 位于 `build/snext-p4-r2/e3.elf`，SHA-256 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。E1/E2/E3完整一致，源码扰动行为已验证。八套最终检查合计405项（Blocking含1条构建检查），另有两系统程序及10平台负例；原91项逐项映射到这份候选的实际证据。

最后的独立组合审计554项通过，报告为 `reviews/integration.md`；系统独立报告为 `reviews/p5-r3.md`。`q/release.json` 保存当前身份、资源、套件、映射和成本索引。没有修改冻结的71个引导输入；当前工作区改动尚未提交。

规模结论保持不变：生产编译器10,991行、325,234字节，超过8,000行软预算2,991行。64MiB资源与1MiB guest栈硬门槛通过。当前交付是RV64 Linux-user自举与有界系统切片，不是SOS内自举或完整操作系统。
