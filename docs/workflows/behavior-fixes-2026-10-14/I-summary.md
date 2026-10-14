# 四项行为修复：组合验证

2026-10-14；实施/集成负责人/root。当前状态：组合机械验证完成；最终独立判定见 [acceptance-r1.md](acceptance-r1.md)。机械通过不替代独立验收。

依赖已独立接受：T-r1、R-r1、D-r2。实施遵循suba/CODE_STYLE.md 0.1，不开展无关格式重写。T/R/D原候选和审查保留。D-r1失败说明与r2根因修复均保留。

## 行为

- 当前维护验证工具在导入前拒绝优化模式，错误结果不能通过消除assert变成通过。新IO/诊断/工具自测使用显式require，正常与优化模式均能拒绝错误。
- 路径复制统一校验NUL。删除与打开一致拒绝，合法前缀不受影响。
- 保存独占创建、完整写入、设置权限、关闭；失败保留主阶段/errno和close/cleanup信息。部分写失败不会chmod；无法删除时明确报告可能残留路径。已有输出不覆盖。
- AST记录字段起点由parser准确保存；检查器和静态初始化显式使用字段/整体span。递归子表达式的诊断及IR位置保持独立。

## 验证范围

完整运行路径为冻结B(P)=E0，E0(Q)=E1，E1(Q)=E2，E2(Q)=E3及源码可观察扰动。随后以同一E3运行语言、输入、机器、Blocking、容量、接口、复制、参考、P5系统切片和本轮新测试。记录目录均为新建build/behavior-fixes-*；旧发布/审计证据不覆盖。全部命令已退出0，证据身份见 [I-results.json](I-results.json) 和 [维护记录](../../../core-s/next/q/maintenance-2026-10-14.json)。独立整体验收另行记录。

成本见I-cost.json：当前Q生产11,018行，较基线+27；VT runtime290行，较基线+89；S0仍19行。增加的是具体保存错误与生命周期、诊断定位，不是新增语言特性。手写工具、测试另列，没有把它们藏入编译器规模口径。

## 限制与后续

仍只证明RV64 Linux-user；K0尚未实施。宿主I/O和输出profile分离、已有公开裸切片合同及统计所有者收紧，按K0相关模块推进，不把AST全面改造作为内核启动前置门槛。

71个冻结种子输入保持原字节。其中verify.py/check_reference.py仍依赖普通Python断言，直接调用不得启用优化；当前维护工具会在导入它们前拒绝优化。未声称修复历史直接入口。

保存不提供并发替换路径安全或突然断电事务；unlink被拒绝时不能保证删除。close失败可能发生在完整写入/chmod之后，残留文件可有可执行权限，但会明确报告失败。故障注入close先执行真实close再返回合成错误，不扩张成所有操作系统失败close语义的证明。

尚未提交本轮修改。

## 本次实际结果

- E1=E2=E3，全文件SHA-256 `8051f3599704a6bf180496b14632691c3463f1239946893641f65aae21969904`。当前ELF：`core-s/build/behavior-fixes-p4-r1/e3.elf`，1,011,792字节。
- 完整B→P→Q链、空guest PATH、1MiB guest栈、输入白名单/源未改、源码行为扰动通过。扰动产物确实输出不同前缀，未只比较两个相同ELF。
- P3准备阶段：126语言案例与11个保留目标测试。最终E3：既有405项 + 新IO12项 + 新诊断18项 = **435项**。
- 最终E3另通过优化保护88次入口/导入检查、5次真实语料调用；P5编译运行两个系统样本、10个监控层反例。计数单列，不把工具进程数混入语言案例数。
- RW静态预留55,135,764字节，映像跨度56,143,380字节；最大单函数frame25,808字节。RW与最大frame相对基线不变。
- 71个冻结来源哈希保持一致。旧发布/成本JSON原字节保留，新产物另有maintenance记录；参考修订3登记新ELF并用该ELF运行全部三个手册例子。

| 最终E3套件 | 通过场景数 | build下记录目录 |
|---|---:|---|
| language | 126 | behavior-fixes-language-r1 |
| input | 24 | behavior-fixes-input-r1 |
| machine | 105 | behavior-fixes-machine-r1 |
| blocking | 48 | behavior-fixes-blocking-r1 |
| limits | 15 | behavior-fixes-limits-r1 |
| interfaces | 76 | behavior-fixes-interfaces-r1 |
| copy | 8 | behavior-fixes-copy-r1 |
| reference | 3 | behavior-fixes-reference-r1 |
| io-failures | 12 | behavior-fixes-io-r1 |
| diagnostics（含真实IR probe） | 18 | behavior-fixes-diagnostics-r1 |

完整复现从core-s目录按README执行check_next→selfhost_next，再使用新e3.elf执行Q README列出的八套既有工具、两个新专项工具、工具优化保护与P5。命令/源码胶囊/日志保存在上述每个verification.json所引用的位置；证据目录必须新建，不覆盖本次或历史结果。
