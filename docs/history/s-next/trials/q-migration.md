# Q 的真实 IR / AST 迁移评估

本评估针对当前 `next/` 的生产源码结构，不是已完成实现。范围只研究实际迁移，不建议永久停留在样本。样本本身增加 6 行 / 521 字节，预测数组减少 136 B；这足以支持继续原型，尚不足以承诺整个编译器净缩减。

## IR：值得直接替换，存储不必增长

当前 `model::Inst` 是 18 个有效 opcode 加 INVALID，带 dst/a/b/c/aux/bits/Span；RV64 预测尺寸 48 B。需要改为 `Inst { span:Span, instruction:Instruction }`，Instruction 首个 Empty 标签用于未分配槽的 zero，后续对应 18 种操作。所有 consumer 直接对其 match，不提供转回 RawInst 的公共或内部兼容解包器。

建议 payload 如下，所有普通 ID 包装为单字段 u32；符号、标签、参数范围和语义槽不能互换。

| 标签 | 具体载荷 |
| --- | --- |
| Value | destination:SlotId, bits:usize |
| Copy | destination:SlotId, source:SlotId |
| LocalAddress | destination:SlotId, source:SlotId, offset:usize |
| SymbolAddress | destination:SlotId, symbol:SymbolId |
| Load | destination:SlotId, address:SlotId |
| Store | address:SlotId, value:SlotId |
| Offset | destination:SlotId, address:SlotId, bytes:SlotId |
| Binary | destination:SlotId, left:SlotId, right:SlotId, operator:BinaryOperator |
| Unary | destination:SlotId, operand:SlotId, operator:UnaryOperator |
| Cast / Bitcast | 各自标签；destination:SlotId, source:SlotId |
| Bounds | index:SlotId, length:SlotId |
| Call | result:Destination, callee:DeclId, arguments:ArgumentRange |
| Label / Jump | 各自标签；LabelId |
| Branch | condition:SlotId, yes:LabelId, no:LabelId |
| Return | Destination |
| Panic | 无载荷 |

`Destination { Unit, Slot(SlotId) }` 明确表示无结果，避免把 SlotId(0) 同时称作有效槽和 unit。`ArgumentRange { first:ArgumentId, count:u32 }` 对应现有连续实参表，不新增调用参数对象图；范围仍由 IR owner 验证。BinaryOperator/UnaryOperator 可采用无载荷枚举，它们不是 arena ID。

成本预测：Value 和 LocalAddress 载荷 16 B、align 8；Call 载荷 20 B、align 4。payload 区按 8 对齐占 24 B，Instruction 为 32 B；前面的 Span 占 12 B、填充 4 B，因此新 Inst 仍为 **48 B**。65,537 项预留仍为 **3,145,776 B**。这比宣称去掉复用字段就能省内存更可信。若继续用带零哨兵的单字段 ResultSlotId，能把 Inst 降到 40 B，但会重新引入“这个 SlotId 为什么不能索引”的特殊合同；本轮不建议为约 512 KiB 优先做此妥协。

包装必须保持 u32。把 DeclId、SlotId 等统一改成 usize 会让 Call 载荷显著膨胀，不能把小样本的低成本外推到该版本。

## 存储拥有者和消费者

1. **IR 模块真正拥有函数存储。** 将 `model` 中 slots/insts/args、计数器及 add/reset 入口移入 `ir`，同时把 Instruction、SlotId、LabelId、ArgumentId 定义在那里。model 保留稳定声明/类型/符号。`ir` 可以依赖 model/types，model 不反向依赖 ir，从而不产生导入环。不要建立另一个 model→ir→model 的 ID 依赖环。
2. **构造器发出具体变体。** 现有 18 类 `ir::value/call/...` 外形可以延续，但参数和返回改成名义 ID；删除公共的七个数参数 `emit`。私有 append 只接收完整 Instruction，加当前位置，并维护明确的终结状态。
3. **后端读已封口函数。** 提供 `begin_function(DeclId)`、`finish_ir()`、只读 instruction/slot/argument 获取、`end_function()`。rv64 不能取得数组可写入口。ID 的索引解包只能通过 owner 的带边界检查接口完成，不回退到公共 `.raw`。
4. **验证不删除。** 把 `rv64::validate` 的 18 类 if 链改为直接 match；参数/返回类型、地址范围、符号种类、标签唯一性、实参范围、槽前置条件继续检查。enum 只消除不合法 opcode/载荷配对，并不证明 IR 语义合法。
5. **发射不保留通用字段。** `binary` 接 Binary，`call` 接 Call，`store_instruction` 接 Store，`return_instruction` 接 Destination；发射每个 match 分支显式保存其目的槽。删除函数末尾依赖 `item.dst` 的公共 store，因为很多分支本来没有 destination。
6. **Blocking 只获取稳定调用关系。** 可以直接穷尽 match Instruction，Call 分支读取 callee，其他标签为空分支；也可以由 IR owner 提供只读 `next_call`。无通配分支的语言合同意味着过滤器必须列完标签，不能偷偷增加“其它 opcode”后门。不要为了省这一份分派而新增未验证、可能和 IR 失同步的调用侧表。

在 `rv64::function` 中，目前还有两种数值约定必须同步删除：`inst.op >= IR_JUMP` 判断终结，以及首指令 `.op/.a` 判断 Label(1)。改用具名标签匹配；标签定义/引用及最大外出参数区的扫描仍是一遍线性扫描。若把标签/终结/Call 信息合入验证器返回的小型摘要，只能返回这些结构事实，不能把所有 operand 又搬进一个通用 record。

## 需要改动的实际范围

| 文件或边界 | 当前必须触及的部分 | 风险 |
| --- | --- | --- |
| model | Inst、Slot、三个数组与 add/reset；稳定 DeclId/TypeId/SymbolId 入口 | 不能把同名 raw u32 带过包装边界后仍允许任意构造 |
| ir，当前约 214 行 | 几乎所有构造器，封口/读取/生命周期；移动存储定义 | 规模增加首先来自显式类型声明和私有访问器 |
| rv64 | validate；binary/call/store/return；instruction；function 中结构扫描 | 是主要行为风险；保持现有指令/ABI结果逐项验证 |
| check | Binding.slot、Place.address、Loop 标签、临时值返回、PreparedCall、cleanup | 当前约 148 个 ir 调用/状态使用处；SlotId 不能仅在 emit 前临时包一下 |
| blocking | capture 中一次指令扫描 | 只允许稳定 DeclId/Span 逃离函数生命周期 |
| main 与测试工具 | begin/finish/end 调度；机器坏 IR 用例的构造方式 | 坏 IR 测试仍要能制造语义错误，不要求恢复 RawInst |

估计约 **700–1,100 行需要修改或移动**，其中大量是参数类型和 match 替换，不能把这个数当作净增行数。若范围仅是上述真实 IR + 名义 ID + 访问器，暂估净增 **100–300 行**，须实际写完统计；世代检查、AST 和具体 I/O 替换另计。全编译器 8,000 行预算意味着需要同时兑现旧 Result/旧 AST 路径等删除，不能宣称“这次只是几处机械替换”。

函数临时 NodeId/SlotId 的低 16 位索引、高 16 位世代适用于当前 32,768 容量及单次编译最多 8,192 个声明/函数的范围。Label 的当前容量 65,536 **不适用**相同 16 位索引：可采用低 17 位索引、高 15 位世代，或将上限改为 65,535 并公开变更。Argument 当前容量同样为 65,536，空范围还可能持有末尾后一位，因此也需要 17 位索引。实际 Q 可统一选择低 17 位索引/高 15 位世代，仍覆盖单次编译 8,192 函数的上限；本小样本的 16/16 只是其自身合同。不要未经检查把同一打包模板用于全部 ID。稳定 TypeId/DeclId/NameId 不需要函数世代。

## AST：直接替换同一 arena，避免再做一份 HIR

现有 `parse::Node` 预测 40 B：tag 与 a/b/c/d/next/start/file 共 8 个 u32，再加 usize bits。它既装表达式，也装语句和 match/pattern 辅助节点。建议沿用一个 arena，但变为 `SyntaxNode { span:Span, next:NodeId, syntax:Syntax }`；Syntax 是覆盖真实语法构造的具体 enum，取消 a/b/c/d 的复用。Span/链表 next 是跨所有标签的元数据，不能藏进某几个载荷。

这里的 name/type-syntax/expression/statement 引用按实际身份命名。Literal `{ bits:usize, suffix:u32 }` 预测 16 B；多数 Binary/If/Bind/Call/构造载荷在 16 B 内。若最大载荷保持 16 B、align 8，Syntax 为 24 B，外部 Span+next 为 16 B，总体仍为 **40 B**；这只是逐个载荷盘点的预算，增加某个含大结构的 payload 会使整个 32,769 项 arena 一起膨胀。

不要再加一层 ExprSyntax→HIR，也不要定义 enum 后给 checker 一个 `legacy_node()` 回收 a/b/c/d。parse、check、受限常量求值、静态初始化都直接读具体载荷。最先核对 P2 HeaderSyntax 中保存的 NodeId：这些语法树的寿命可能跨函数重置，不能一律附函数世代。声明初始化的持久节点和逐函数节点必须在 API 或 owner 生命周期中区分，避免合法头部引用被 begin_function 销毁。

统一 Syntax 的代价是 expr/statement/constant 各自需要显式拒绝不属于该入口的标签；闭合 enum 无 wildcard 时，拒绝分支会占一些源码。先测量这种显式程度，别为追求最少分支立刻拆成三套同容量 arena。若拆 Expr/Stmt/Pattern，必须重新分配总容量而非默认各加 32,769 项，并公开不同类别容量耗尽的限制。

## 整合顺序和验收

先完成 enum/私有字段/zero 本身的独立正反例，再建立 Q 的真实 IR 类型与 owner，贯穿 check→validate→emit→Blocking，删除旧 Inst 和所有 opcode 数值范围判定；随后直接替换 AST，而不是在 P 和 Q 永久维护两套新结构。

每个表示替换后都需独立检查：相邻函数、旧 ID、容量耗尽、非法槽类型/参数范围/标签、聚合与超寄存器 ABI、Blocking on/off 输出一致。最后重新做 Q 完整固定点和源码扰动。小样本的 42 与 sizeof 通过只能证明试点，不能替代这些主线验收。
