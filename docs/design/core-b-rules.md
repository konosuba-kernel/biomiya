# Core 0.6 B（T1、T2）规则草案

状态：R 节点候选，尚未独立验收或实施。本文是相对 [reference 0.5](../reference.md) 的完整增量规则；现行 reference 与产品编译器保持 0.5。选择依据为 [D3](../../build/simplification/D/options.md)、[D3 独立审查](../../build/simplification/D/review-r3.json) 与 [L1](../../build/simplification/L/choices.md)。D3 的构造例外优先于 L1 的所有泛型值路径显式方案。

仅替换 §6.12 的求解式推导、§10.1／11.3／附录 D 的泛型实例省略规则，并限制 §6.5／11.1／26 的泛型依赖常量用途。未明示修改的 0.5 规则继续适用，包括完整递归模式覆盖、聚合相等、方法自动借用／解引用、Option/Result、?、defer、for、资源标记、原始存储、原子、MMIO、地址空间、完整既定 ABI 与 RV64 合同。T3、T4 和 L-E 不采用；Extend 的配置、可用信息及保证边界保留。本草案不新增 trait、const fn、常量泛型、隐式分配或其他语法。

## 1. 检查结果与期待类型

每个表达式检查只有两种入口状态：没有期待类型，或调用方在检查该表达式前已经知道一个确定的类型。实例内该类型须具体；模板内允许以已声明参数构成的确定类型结构，如 T、Option<T>，它们是参数引用而非可求解的未知变量。返回确定类型／参数化类型结构或 `!`；不产生整数推导变量，不保存未解的泛型实参，不回传同级约束，不按源位置回灌类型。

期待类型是检查依据，不能改变一个已经具有类型的路径／调用结果。例如 `let x = 1; let y:u8=x;` 仍拒绝；y 的声明不能重新定型 x。整型间没有隐式转换。保留 §6.12 列出的适配，仅在已知目标类型的位置执行；无期待的分支／数组／break 不寻找共同超类型。具体接收者的自动解引用及方法借用继续由 §11.9 决定。

期待类型传播如下；未列出的子表达式独立综合自己的类型。

| 来源／表达式 | 给子项的期待类型 |
| --- | --- |
| 显式 let/var、const/static 类型 | 完整初始化器使用声明类型；模式在初始化器确定后检查 |
| 无类型 let/var | 初始化器无期待；结束即冻结绑定类型。解构不从绑定的后续使用补类型 |
| 函数体尾部、return | 已声明返回类型；return 的类型本身为 `!` |
| 赋值／复合赋值 | 目标位置已确定的值类型给右部；仍保留 §8.3／12.6 的运行求值与快照顺序 |
| 已显式实例化函数、普通函数、函数指针调用 | 先确定签名，再给每个实参相应形参类型；调用结果期待不决定实例 |
| 块、括号、unsafe 块 | 原期待只传尾表达式／内表达式，不传前面的无注解局部初始化器 |
| tuple、数组、重复数组 | 若期待形状相符，分别给分量／元素类型；长度仍精确检查。否则子项无期待 |
| 结构体／newtype／enum 构造 | 先按第 3 节选定实例，使用替换后的字段／载荷类型 |
| if、match | 同一个原期待给每个结果块／臂；条件为 bool，match 被检值独立检查，模式由其类型决定 |
| loop | 进入 loop 前的原期待给所有指向它的 break 操作数；嵌套循环维护独立目标 |
| while、for | 类型 ()；条件为 bool、输入独立检查，循环体按 () 检查；只允许裸 break |
| 算术及整数位运算 | 仅当原期待是整数类型时，给两个操作数同一个整数期待；否则二者各自无期待，最后核对类型一致 |
| 检查算术 +?／-?／*? | 原期待为 Option<I> 且 I 为整数时给两个操作数 I；没有该期待时各自定型。结果为 Option<I>，不从一个操作数补另一个 |
| 移位 | 左项仅接收原整数期待；右项独立检查，保留 §12.2 明确的无后缀计数字面量按左项类型检查这一固定操作规则，不推广到其他算术／任意右表达式 |
| 一元整数运算 | 原期待为整数时传入操作数；保持负字面量最小值合法处理，不先拒绝其正幅值 |
| 比较 | 结果 bool 不能决定操作数；两个操作数各自无期待，再核对同类型与比较能力 |
| &&、\|\|、!（逻辑） | 操作数 bool；没有新增整数逻辑规则 |
| 字段／下标／解引用／方法接收者 | 基址独立定型，结果期待不反推基址／所属实例；索引独立检查其整数能力 |
| 范围切片 | 基址独立定型，端点各自定型并核对同类型；结果期待仅选择既有可变／只读视图适配 |
| as | 源独立定型，检查到显式目标的既有转换；目标不反推源整数类型 |
| ? | 操作数独立定型为 Result<T,E>，与函数错误类型核对；结果 T 可在外围已知目标处按既有适配检查 |

不以“左边先检查”偷偷补右边的期待，也不以第一分支／第一元素／第一 break 为后面建立期待。访问字段时仅读取所选字段，不能因取消推导而引入整个 non-Copy 聚合读取；L1 记录的显式解引用误拒按规范纠正。

## 2. 默认、空值与控制流合并

无后缀整数字面量在当前节点已知整数期待时采用该类型，否则立即采用 i32，并按该位宽检查范围。显式后缀固定类型；已知非整数期待不会把字面量变成该类型，最终不匹配。长度表达式按 §26 的非负整数常量检查；它不是强制 usize 的新隐式转换点。既有特定形参（如 usize）照常向实参传播期待。

`null` 仅在当前已知原始指针期待时合法；地址空间、目标与 const 性来自该类型。不向引用／函数指针转换，也无默认指针。`[]` 需要当前已知 `[0]T` 期待；非零长度不匹配；不能构造成切片。重复数组零长度仍检查且求值元素一次，元素仍需 Copy。无期待的非空数组每个元素独立定型并严格相同；若全部元素是 `!`，不得把 `!` 当元素类型，须已有合法元素期待。

有 else 的 if、match、loop 的全部正常结果必须严格同型；`!` 不贡献正常结果。无原期待时只比较已综合结果，不把比较得到的类型传回任何分支。全部 if/match 结果为 `!` 时整体为 `!`；缺 else 的 if 为 ()，then 须 () 或 `!`。match 覆盖、可见性、借用／拥有匹配保持 0.5。

loop 收集全部词法上指向它的 break，包括常量条件不成立及不可达代码中的 break。裸 break 提供 ()；带值 break 的 `!` 操作数不贡献正常退出类型。没有正常结果 break 时 loop 为 `!`；所有非 `!` break 必须同型并满足进入 loop 时已知期待。不能利用后一个 break 帮前一个 null／空数组定型。continue 为 `!`；while/for 仍为 ()。块对不可达语句逐项检查，但其尾部不创造正常路径，保留 §11.2 的结构可达性。

```text
// 接受：外部已经提供完整类型
let a:u64 = 1 + 2;
let b:[2]u8 = [1, 2];
let p:*u8 = if flag { null } else { ptr::<u8>(0) }; // ptr 调用仍须 unsafe
let xs:[0]u8 = loop { break []; };
let k:u8 = loop { if flag { break 1; } break 2; };
let value:u8 = if flag { return 0; } else { 3 };
// 拒绝：同级不能补类型
let a = 1 + 2_u8;
let b = [1, 2_u8];
let c = if flag { 1 } else { 2_u8 };
let p = if flag { null } else { ptr::<u8>(0) };
let xs = [[], [0_u8; 0]];
let k = loop { if flag { break 1; } break 2_u8; };
// 比较无整型期待，须写 1_u8；bool 只描述比较结果
let same:bool = 1 == 2_u8;
```

上例为语句片段，需在 flag:bool、匹配函数返回类型及相应 unsafe 上下文中使用；不是独立可编译 fixture。

## 3. 函数、方法与构造实例

用户泛型函数、函数值路径和关联函数要求每个相关泛型路径段的完整类型实参，不能全部省略、部分提供或用占位符。期待函数签名也不决定实例。普通非泛型段禁止多余实参。方法的所属类型来自已确定接收者，方法自己的类型参数必须完整显式；类型路径形式须显式所属参数。泛型 impl 内 `Self` 或已经包含其已绑定参数的确定类型路径不算省略。

构造例外只用于真正的名义结构体／newtype／枚举构造语法：当表达式当前期待已经唯一指定同一声明的完整名义实例时，可省略该构造者的全部所属实参，直接读取期待中的替换表，随后按字段类型检查。省略例外要求期待实例不含定义环境的参数。模板内 `Option<T>` 虽有确定类型结构，仍须写 `Option::<T>::Some(x)`；这既不从载荷猜测 T，也不等替换后的具体实例再放行省略。模板中的 `Option<u32>` 则可省略。通过透明别名展开可取得这个实例；括号、块、分支可传播该期待。期待的 tuple／reference／Result 包装不能凭空穿透到任意内部调用，只按第 1 节结构传播。

解析仍按名字身份与可见性，不能按拼写 Some、None、new 猜测构造者。显式实参始终先形成完整实例再与期待核对；不同声明／参数数目／实例不匹配不能选择另一个候选。模式继续由被检类型确定实参，不新增模式 turbofish。

```text
fn id<T>(x:T)->T { x }
let x:u8 = id::<u8>(1);               // 接受，形参给 1 期待
let bad:u8 = id(1);                  // 拒绝，返回期待不选实例
let f:fn(u8)->u8 = id::<u8>;          // 接受
let bad_f:fn(u8)->u8 = id;            // 拒绝
let p:Option<u32> = Some(3);          // 接受，真实构造者
let n:Option<u32> = None;             // 接受，无载荷也可直接选实例
let q = Option::<u32>::Some(3);        // 接受
let bad_q = Some(3_u32);              // 拒绝，载荷不推导实例
let atom = Atomic::<u32>::new(0);      // 接受，关联函数
let bad_atom:Atomic<u32> = Atomic::new(0); // 拒绝，非构造语法
```

附录 D 普通泛型内部接口也完整显式：replace、swap、forget、load/store 类、slice_from_raw_parts(_mut)、split_at_mut 等按其真实声明提供 T。包装器关联函数 MaybeUninit::uninit、UnsafeCell::new、Atomic::new 要求所属实参；已确定接收者的 get/read/write 等成员无需再写所属 T，不接受不存在的方法参数。

固定签名的内部例外保持闭合：addr、addr_of／addr_of_mut、ptr_add／ptr_add_bytes 由已定型输入／位置唯一确定相应类型，禁止显式实参；checked_cast::<D>(x) 只写目标类型，源由 x 定型；ptr::<T, space> 保持其专用语法及默认 kernel 地址空间。panic 等非泛型接口与 sizeof／alignof／offsetof／static_assert 语法内部函数不改。无任何内建以外围结果倒推实例的附加例外。

## 4. 模板与实例验证

每个定义先检查词法、语法、名字、可见性、参数数目、显式实例完整性，以及 T2 的定义依赖禁令；未使用模板也不能逃过这些检查。模板中无泛型依赖的基础类型错误照常拒绝。仅因实际 T 的能力／具体布局才能决定的操作仍按 §10.2 在所需实例检查，不要求证明泛型体对所有类型成立。

例如 `fn unused<T>(x:T)->T{x+x}` 保留 0.5 的未使用模板接受行为；调用 `unused::<u32>(...)` 可以合法，`unused::<bool>(...)` 在具体运算处报 E_TYPE 并附实例路径。不能把一个“检查未知 T 的 +”的新 trait 系统加入定义检查。`fn bad<T>(){missing;}`、错误显式参数数量及依赖长度即使未调用也拒绝。

模板与实例使用同一表达式规则，参数替换后重新验证实际能力；允许记录明确的参数化能力义务／实例来源，不允许以通用未知量求解模拟取消的推导。保留声明身份＋完整类型实参缓存、相同实例递归、多态递归拒绝、Copy 约束、具体类型检查、不可达具体调用检查及定义模块的可见性。泛型普通表达式中的 sizeof(T) 等查询在实例布局已知时定型／降低，不能宣称它们在定义点已经得到数值。

## 5. T2：定义参数依赖与提前常量值

在所有数组类型长度、数组重复构造长度以及所有 static_assert 条件中，表达式不得直接或间接依赖其定义环境的任何泛型参数。位置包括字段、类型别名右部、函数签名、局部注解／初始化器、嵌套类型实参、impl／函数体及常量块。禁止在定义检查时生效，不能等实例替换成具体类型放行；未使用定义也检查。不是取消泛型字段／固定长泛型数组：`struct A<T>{items:[4]T}` 保留。

依赖分析是按名字身份运行的保守结构检查，与按值布局需求图分开。一个提前常量根的依赖集合取其完整语法／已解析声明依赖的并集：

1. 类型中出现参数，包括类型查询参数、显式实例实参、局部注解、转换目标、构造类型和模式需要的参数化类型，都贡献该参数。透明别名递归展开，不能只扫描根文本。
2. 读取名字沿初始化器／定义依赖传播；常量块局部 let 沿其绑定初始化器与注解传播。任何含参数的中间绑定、聚合构造、投影／匹配都不能清洗依赖，即使最终尾值为固定 1。
3. 遍历所有子项及所有分支，包括 dead branch、未执行语句和逻辑短路项；不执行程序证明参数“没影响最终数值”。例如 `if true{4}else{sizeof(T)}` 拒绝。
4. 固定间接类型不能清除 T2 的定义参数引用（有意采用简单、保守的定义规则）：`sizeof(*T)`、`sizeof(fn(T)->())` 在这些根中拒绝，即使机器表示固定。这是定义语法依赖禁令；普通布局需求图仍按 §10.3 在固定间接处停止，不可互换两个分析。需要固定指针表示尺寸时可以使用 `sizeof(*u8)` 或既有具体常量。
5. 经 const／类型别名及嵌套数组长度的间接查询继续遍历；非泛型全局 const 不可能合法捕获未绑定 T，但仍需解析及依赖循环检查。循环不因死分支删除，常量／布局循环按既有 E_LAYOUT 规则诊断。

先解析与检查基础语法／名字，再检查定义依赖，随后执行普通常量允许性与求值；各阶段独立失败类别保留，不规定同时含多个错误时唯一报错顺序。纯 T2 禁令使用 E_CONST、定位提前常量根并附参数及依赖链；一般类型错误 E_TYPE、布局循环／溢出 E_LAYOUT、实例约束 E_INSTANTIATION 保持。不得把实例内部所有错误粗归 E_INSTANTIATION。

```text
struct PteScratch { bytes:[sizeof(Pte)]u8 } // 接受：具体类型
struct Fixed<T> { values:[4]T }            // 接受：元素可泛型
fn size<T>()->usize { sizeof(T) }          // 接受：普通表达式实例时查询
fn guard<T>() { static_assert(4 == 4); }   // 接受：条件与 T 无关
struct Bad<T> { bytes:[sizeof(T)]u8 }      // 拒绝：直接依赖
struct Dead<T> { bytes:[if true {4}else{sizeof(T)}]u8 } // 拒绝
struct Indirect<T> { bytes:[{let n=sizeof(T);n}]u8 }    // 拒绝
struct FixedPointer<T> { bytes:[sizeof(*T)]u8 }        // 拒绝：定义引用
fn bad_repeat<T>() { let bytes=[0_u8;sizeof(T)]; }     // 拒绝
fn bad_assert<T>() { static_assert(sizeof(T)>0); }     // 拒绝
```

上述长度为非负整数常量，具体位宽仍由第 2 节确定；混合 i32 与 usize 运算需要显式后缀／转换。非泛型 `static_assert(sizeof(Pte) <= 64_usize)` 与具体类型的 sizeof／alignof／offsetof 常量计算保留。泛型普通控制流可写 `if sizeof(T)>64_usize {panic("large");}`，它有运行控制流合同，不能冒充编译期断言等价迁移。非泛型常量块的 if/match、聚合、静态初始化、地址重定位与包装器合法初始化继续保留；检查全部分支允许操作，执行只走选中路径，dead division 不执行，dead 普通函数调用仍拒绝。

## 6. Fixture 分类与迁移矩阵

下面是已读取用例和定向扫描测试名称形成的迁移清单，不是已运行的新版本测试结果或全仓接受范围证明。0.5 fixture 与 manifest 保留原义；后续应建立 0.6 独立版本矩阵。若一个文件混合变化／保留断言，按断言拆分，不能整个删去。

| 现有 fixture／测试 | 0.6 分类与迁移 |
| --- | --- |
| audit/infer_binary、infer_if、infer_match、infer_array、infer_loop_break、infer_branch_tuple、infer_branch_statement | 原依赖同级默认的正例变 E_TYPE 拒绝；加注解或全部同型后缀形成接受／运行对照 |
| audit/infer_array_empty_from_peer | 原正例变拒绝；外层 `[2][0]u8` 注解后保留空数组接受 |
| audit/infer_generic、infer_generic_result_context、infer_match_constructor | 省略调用／构造实例变拒绝；显式调用参数与已知名义构造期待分开验证 |
| audit/generic_constructor_inference、generic_enum_inference | 无期待 W 构造变拒绝；W::<i32> 显式构造及 `let x:W<i32>=...` 保留例外正例 |
| audit/builtin_generic_expected_literal、replace_explicit_mismatch、forget_explicit_mismatch、volatile_explicit_mismatch、nongeneric_intrinsic_type_arguments | 显式形参期待和错误实参核对保留；禁止实参的内建仍拒绝；任何省略普通泛型内建新增拒绝 |
| audit/method_generic、generic_impl、generic_explicit_allows_weakening | 已显式所属／方法实例保留；省略方法自身参数的子项迁移；已具体目标的弱化正例保留 |
| tests/inference.rs structural_constraints_precede_literal_defaults | peer tuple／branch／array／break／call 正例变拒绝；字段 `s.len+1` 也需 `1_u8`，不暗加左到右推导；`!` 无正常结果与已知赋值目标子项保留 |
| tests/inference.rs inferred_constraints_are_strict_and_prior_locals_are_hard | 先前绑定不反推与无目标分支弱化拒绝保留；缺显式 id 调用增加缺实参拒绝；明确 &i32 目标的再借用保留 |
| tests/inference.rs constant_branch_prefixes_keep_independent_bindings_and_soft_tail_peers | 非泛型常量 prefix 及固定长度保留；soft tail peers 的混合 tuple 改拒绝／加完整期待 |
| audit/unused_generic_dependent、generic_dependent_unused、unused_generic_name、unused_generic_bad_type、generic_bool_arithmetic、generic_integer_condition／logic | 名字／语法定义检查及依赖能力实例边界保留，不能一律拒绝未使用模板；显式调用迁移独立记录 |
| audit/array_dependency_const、runtime_let_array_length | 非泛型依赖循环拒绝、运行值非常量拒绝保留 |
| tests/const_constructors.rs deferred_offsetof_retains_definition_module_visibility | 依赖 T 的长度优先形成 T2 拒绝对照；另加具体 offsetof 保留定义模块可见性断言 |
| tests/const_constructors.rs deferred_dependencies_include_queries_nested_in_type_arguments_and_annotations、by_value_constant_constructors_participate_in_declaration_dependencies、nested_length_queries_replay_concrete_lexical_prefixes | 含 T 的原接受／重放例改定义点 E_CONST；包括 f::<*B<T>> 固定间接例。非泛型死分支循环、构造布局依赖及可见性独立保留 |
| tests/layout_abi.rs symbolic_layout_constants_substitute_and_dead_branch_dependencies_remain、dependent_checked_operations_preserve_widths_and_operand_errors、checked_length_conditions_substitute_query_operands、checked_deferred_instances_substitute_independently_and_survive_cloning、deferred_queries_preserve_dead_dependencies_and_fixed_indirection_stop | 泛型依赖长度／提前断言部分迁移为源定义拒绝；具体长度等值、整数错误、常量死分支、固定间接布局停止与克隆来源信息保留独立断言；内部 Deferred 构造测试按新接口重写 |
| tests/layout_abi.rs finite_nested_generic_and_growing_recursion、indirect_generic_parameter_does_not_require_layout、concrete_symbolic_array_lengths_compare_by_value、packed_alignment_and_atomic_legality | 有限泛型布局、固定间接普通布局、具体常量与 ABI 能力保留 |
| tests/extend.rs checked_generic_lengths_survive_lowering_and_target_snapshot_clones | 不再用禁止的泛型长度当输入；具体常量长度、固定 `[4]T` 及泛型普通查询提供替代；必须继续验证语义／lower／target clone 与实例来源，不删弱 Extend 断言 |
| audit/match_nested_shared、enum_equality、mutable_reference_equality、for_move_array、array_zero_sideeffect、loop_break_value、defer_break_value；tests/backend.rs、integration/、raw-storage 与 ABI 测试 | 对应未选删减的能力保持；仅存在 T1/T2 写法时迁移写法，求值快照、清理顺序、运行语义原断言保留 |
| L/evidence/implicit.vt、explicit.vt | 按 §7.1／11.9 二者都应接受 Copy 字段投影；旧编译器 explicit 误拒列为规范缺陷，不能计成有意收紧或要求新检查器复现 |

泛型依赖存储的迁移须由具体用途选择：具体专用类型可保持选定实例布局；固定容量可能改变大小、容量与 ABI；切片增加借用有效期与拥有存储责任，不等价于拥有数组。增加泛型类型实参与后缀是显式源码成本，应连同编译器新检查／helper 计数。不能仅按名字文本替换 Some／new，也不能改变求值次数或将聚合相等迁为填充字节 memcmp。

## 7. 实施和验收边界

R 独立审查应逐项验证第 1 节传播表与第 2–5 节正反例、未使用模板和实例验证边界、T2 死分支／别名／固定间接依赖及所有保留能力。P/V 的受限范围与共享接口由同节点另一文档确定；不支持的原型语法必须明确拒绝，不可冒充 Core 拒绝。最终全量 0.6 需要规范、manifest、fixture 与产品实现同步，但本次只提交草案。

代码净减少、理解改善、维护任务成本、编译／运行性能均未测得。现有 inference／deferred 模块行数不能当成可全删行数，亦不能承诺 9,000／5,000 行目标。此规则候选的自查不构成独立验收，不能自行解锁 P/V。
