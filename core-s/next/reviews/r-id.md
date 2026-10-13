# R-ID r2 独立验收

2026-10-13，验收者 `/root/resume_review`。未参与生产源码和正式测试工具实现；只读审计并在独立 build 目录制作探针。

**R-ID 合同通过。** 这只接受稳定 NameId/TypeId、Type 字段职责分离、重复语法解包收拢和递归布局位置修复，不提前接受 R-Owner 或整体 P4。

## 候选身份

- `build/q-stableids-r2/compiler.elf` SHA-256：`1e4b4c15f3bd613652901364e3ec8a2e5b0e04a5cbe278f3a1948e8f8e0942b0`，987,216 字节。
- `build/q-stableids-r2/inputs.json` 列出 **18** 个输入，集合身份 `896328f71fd5f63e0232db25eaa7fb7a78dae5bcc6ed340bfaad2eb83bd07006`；逐项核对当前文件 SHA-256，结束时仍全部相符。实现者消息称 19 项属于计数口误，实际证据与 manifest 为 18 项。
- 构建使用已接受行为证据的 r1 E3 驱动；本节点没有声称新候选已完成多代固定点。R-Bootstrap 必须重新完成 B/P/Q 全链。

## 静态边界

NameId/TypeId 私有 raw 已实际进入 AST payload、Header、Field、Param、Decl、Type、IR Slot、常量和检查接口；没有仅增加包装而继续以裸整数传递名字/类型的平行通道。kind 仍是数值判别，与 TypeId 明确分离；primitive_type 只允许 0–5（0 为无后缀/空类型哨兵），其他类型由池添加。表索引转换检查范围，没有公共任意整数→NameId/TypeId 构造。

`Type.element: TypeId` 与 `Type.owner: u32` 已分开；指针/数组/切片跟随元素身份，结构/枚举准备跟随声明身份。Decl/Symbol/Field 等其他稳定索引仍是 u32；此次不声称所有 ID 类别都已由语言类型区分。Name/Source/Image 表私有性属于后继 R-Owner，当前公开状态不在此豁免。

`parse::name_of`、`parse::field_value` 收拢具体解包；Defer 直接携带已校验 Call，checker 不再次完整匹配节点判断是否调用。未使用的 AST_* 常量删除，没有恢复 RawNode 或万能 visitor。

数组布局定义位置由 TypeId 对应的 layout_origins 保存；递归布局返回后恢复该 Span，再检查大小。结构字段大小聚合前恢复字段 Span。语义模块不写 lexer token/cursor。模块私有 diagnostic 上下文本身不违反方案§5；原文不要求删除全部 locate 或为所有内部函数增加 Span 参数。终端 `model::diagnose(code, span)` 的主线后续调整仍需最终组合核对，不能提前写为本候选已有。

## 独立执行：13 项全部通过

命令和源见 `build/r-id-independent-r2/probe.py`、`results.json`；绝对 qemu、空 PATH、1 MiB guest 栈。探针直接导入冻结候选的真实 model/runtime。

- NameId→TypeId、TypeId→NameId、kind 数字→TypeId、数组内 NameId→TypeId：E_TYPE，均在调用者文件 4，无产物。
- 私有 NameId/TypeId raw 构造、私有 TypeId 的 zero、读取 NameId.raw：E_NAME，均在调用者文件 4，无产物。
- 名字驻留相等、合法 primitive TypeId、类型池受检遍历：生成程序实际退出 42。
- 第二文件的缺失名字、错误返回类型、未授权解引用：精确定位对应源 token。
- 第二文件 Outer 数组在递归准备后超限：`actual=67108865 limit=67108864 I_LIMIT file=2 byte=30`，精确指向 Outer 的 `[`，没有借用 Inner 的后续位置。

另核对作者合法指针数组递归、枚举、defer 快照样本。作者初稿把数组长度常量写为 u32 被正确拒绝，修正 usize 后实际退出 42；原失败与修正证据均保留，不能把夹具错误称编译器缺陷。

## 成本和后继门槛

本候选 14 个生产 VT 为 **10,906 物理行、319,418 字节**。相比 r1 的 10,549 行、316,311 字节增加 357 行、3,107 字节；8,000 行软预算仍未达到。语法重复减少与身份检查新增同时存在，不能据局部删除宣称全体规模缩减。

允许 R-Owner 以此输入继续。R-Owner、新固定点、完整保留规则专项、更新参考身份、P5 新候选重验以及 Integration 尚未完成，P4 r1 总体拒绝不因本子节点通过而自动撤销。
