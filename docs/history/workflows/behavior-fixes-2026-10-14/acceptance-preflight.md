# 组合修复只读预审（不是 A 验收）

2026-10-14；审查者 `/root/behavior_fixes_acceptance`，未参与 T/R/D/I 实施。

**预审观察：尚未发现需要退回 T-r1、R-r1、D-r2 的阻断问题。I 尚未冻结，本记录不接受组合发布，不证明当前源码已完成固定点或全套回归。**

## 已检查的输入

阅读 DELIVERY、suba/CODE_STYLE.md 0.1、原审计 F1–F4、T/R/D 最新候选及独立验收说明、完整生产差异、新增三个验证工具与对应 VT/S0/JSON 反例。只读检查相关调用上下文：parser 的 FieldValue 创建、checker 的 expr_inner、static build_inner，以及保存链路和故障 shim。

直接重算候选记录内的文件哈希：T-r1 14 项、R-r1 7 项、D-r2 11 项（含 evidence）全部匹配；seed.json 的 71 项冻结来源全部匹配。本次未修改生产源码、工具、语料、发布记录或旧证据，没有重复完整自举，也没有把已存在的通过记录冒充本审查者重新执行。

## 根因与合同核对

1. **F1：**11 个维护入口在其他导入之前通过 `if not __debug__` 显式终止，脚本调用和模块导入均受到约束；没有用另一个 assert 保护 assert。selfhost 的保护由 main 上移至模块入口。测试以真实编译器执行实际返回 0 的程序，分别要求 0/7，能够区分正常通过与错误期望；优化模式另外要求没有创建证据目录。冻结 verify/check_reference 的直接优化入口仍有历史限制，已明确列出，不能对外宣称所有历史工具都修好了。
2. **F2：**open/remove/save 共享 copy_path；空串、长度上限、NUL 在 syscall 之前检查。私有 copy_path 的全部调用点均提供新建零填充 97 字节数组，末尾 NUL 与目标范围前提成立。原 open/remove 公共错误码保持不变。
3. **F3：**save_new 独占创建成功后才拥有 fd/清理责任；保存顺序为 write→成功才 chmod→恰好一次 close。write/chmod 的最早 errno 不被 close 替换，close 失败仍记录，unlink 的额外错误独立保留。创建失败不会删除既有输出。close 不重试；故障 shim 先真实关闭再合成错误，避免把仍打开 fd 的假失败当作 Linux 语义。input 层报告 path/phase/errno/close_errno/cleanup_errno，在清理失败时标出 residual；stderr 失败的忽略有明确理由。新增 unsafe save_new 注明同步借用及独占路径前提，不引入并发替换或持久化保证。
4. **F4：**parser 在读取字段 ident 前保存起点，修正了字段 span 本身；只在 check/init 传已有 span 的 r1 不足以修复，失败记录被保留。字段存在性/重复/权限直接使用字段 span，整体/遗漏使用 record span。子 expr/build 先执行，成功后才恢复父上下文，因此子错误不被抢先改写；局部构造的后续 Store 使用父 span，而已生成 Bounds 保持子 span。没有修改字段匹配、类型检查、可见性或求值顺序。

## 反例的区分能力

- IO 测试同时核对 syscall 事件顺序、磁盘字节/权限和诊断，不能只靠被测返回值自证成功。覆盖正常、既有文件、短写、零进展、部分写失败、chmod/close 失败以及清理复合故障。NUL 用真实保留的前缀文件核验未删除。
- 诊断测试同时跑 check/build，核对精确类别、文件序号与 UTF-8 字节偏移，并检查失败无输出；有效构造与真实越界退出提供成功/子错误对照。IR probe 调用真实 checker，分别检查子 Bounds 和父 Store 的位置。不是仅比较新增 helper 的镜像实现。
- 三个新增验证工具的验收判断使用显式 require；既有维护工具使用早期优化拒绝。测试没有放宽旧拒绝边界，也没有把新行为通过归因于旧 E3 直接执行旧代码。

## 正式 A 仍需完成

待 I 冻结后，核对同一候选的源码/工具/测试/文档身份，以及 E1/E2/E3 实际文件固定点、可观察源码扰动、完整 suite、三个新增回归与实际消费者记录。需区分旧冻结 E3 的编译引导作用与最终 E3 的行为验收，核对发布说明、规模和限制与实际证据一致。

本预审不提出无关的 AST/InitData/裸切片全面重构；K0 的接口收紧仍按已限定的后续范围推进。
