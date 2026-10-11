# Core S 有界架构原型合同 H r1

本合同按 `core-s-semantics.md` r2 实施原型；不是完整 Core S 支持声明。
0.5 已封存为提交 `8feff82`、标签 `core-0.5-archive-20261007`；新实现位于独立 `core-s/` crate。
H 只冻结共享结构，不实现前端、检查器或后端。只有 H 独立通过后，P/C/B 才正式并行启动。

## 支持范围与拒绝分类

原型处理一个 UTF-8 文件的根模块，无自动入口。支持 u8/u32/usize、bool、unit、kernel 原始指针、具名结构、固定数组和固定 Result。
结构字段具名、声明顺序布局；Result 的 Ok 标签 0、Err 标签 1，标签 u32；所有成员、载荷与长度的具体布局由 P 决定。
每参数、结果、局部及 match 载荷绑定都写类型；支持 let/var 初始化、未初始化 var 槽、普通/unsafe 具体函数、前向与递归调用。
表达式支持整数/布尔/unit/null、具名常量、普通调用、构造、数组重复、字段/数组索引/数组.len、整数运算/as、逻辑短路、Result ?。
仅整数 as，禁止源指针/整数转换；支持 addr_of/addr_of_mut、kernel 解引用、unsafe 块、位置字段写入。取址内建按名字解析身份选择，不按字符串无条件抢占用户绑定。
语句支持块、if/else、while、loop、return/break/continue、浅层 bool/整数/Result match，以及第 6 节限制的块式 defer。
浅层 Result 模式须写 `Result::<T,E>::Ok(name:T)`/`Err(name:E)` 或载荷 `_`，不准递归模式；覆盖/重复按 Core S 位集规则。
常量仅标量（u8/u32/usize/bool/unit）、具名 const、sizeof/alignof、相同白名单整数/逻辑运算/as；数组长度和重复长度期待 usize。
常量及布局收集全部静态依赖（包括短路侧、零长度），按 C/N/L 图拒绝环；不能用求值短路隐藏循环。
所有函数体及不可达语句照常检查；检查器不得因无正常后继跳过诊断。未初始化读的合法性是运行合同，不增加零填充或流敏感证明。

以下是 Core S 合法而本原型未实施的范围，识别后报告 `I_UNSUPPORTED`：模块/导入/pub、type 别名、其他整数类型、用户枚举、Option、切片、函数指针/间接调用、never 函数结果、user/phys/mmio 指针、静态/外部对象、全部属性、FFI、原子/volatile/MMIO/CSR/asm/trap、字符串/源 panic 消息、offsetof/static_assert、聚合/地址常量及复杂常量查询。
Extend 注册/执行/报告接口亦未实施，必须报告未提供；IR 保存源位置、类型和显式存储事实不等于验证器通过。
泛型声明/普通泛型调用、语言引用、impl/方法、tuple、for、块值、带值 break、解构、复合赋值等 Core S 排除项，使用 `E_PARSE`/`E_TYPE`/`E_PATTERN` 等对应规范拒绝，不能伪称只是 I_UNSUPPORTED。
非法名字/重复声明/不匹配类型/不穷尽/缺 unsafe/非法常量/布局环分别使用 E_NAME/E_TYPE/E_PATTERN/E_UNSAFE/E_CONST/E_LAYOUT；阶段允许先发现其中一个真实错误，不固定报错顺序。

## 冻结服务接口及并行写入

crate 不依赖旧 biomiya crate。只有 source.rs、diagnostic.rs、lexer.rs、unicode.rs 通过 path 直接复用；不得调用旧 resolve/types/check/HIR/lower/layout/abi/backend 服务。
serde 仅用于复用 source/diagnostic 的序列化 derive，不能转入外部语义实现。直接复用的手写源码和生成 Unicode 表均纳入复评总账，分列计数。
`lib.rs` 的 ParseService/ResolveService/CheckService/EmitService 是实际函数签名合同，不是 stub 或默认成功路径。

| 节点 | 必须实现的函数 | 写入及独立构建 |
| --- | --- | --- |
| P | `frontend::parse(FileId, &str) -> Result<ast::Program>`；`frontend::resolve(ast::Program) -> Result<model::ResolvedProgram>` | frontend.rs、tests/frontend.rs；`cargo test --manifest-path core-s/Cargo.toml --features frontend --test frontend` |
| C | `check::check(&model::ResolvedProgram) -> Result<model::Compilation>` | check.rs、tests/check.rs；`--features check --test check`，可手构 resolved 输入，不调用 P |
| B | `backend::emit(&model::Compilation) -> Result<String>` | backend.rs、tests/backend.rs；`--features backend --test backend`，可手构 IR，不调用 P/C |
| I | 驱动显式按 parse→resolve→check→emit 串接，并提供执行夹具 | main.rs、fixtures/tools/README；`--features pipeline` |

H 的默认 `cargo check --manifest-path core-s/Cargo.toml` 仅构建共享接口与 lexer，不假装已完成 pipeline；core-s 可执行目标要求 pipeline feature，I 才提供 main.rs。
各 feature 单独打开对应模块，pipeline 汇合三者；节点只 rustfmt 自有文件，不全 crate 格式化共享并行产物。
共享 API 修改必须由 H 增 revision、重审后传播，不能由 P/C/B 各自改字段。Cargo.lock/target 为 cargo 自动产物，非手写实现。

## AST → resolved 的 P 输出保证

ast 保存真实源码位置及未定语法，不保存推导变量或 HIR。Program 的顶层声明顺序决定 FunctionId；仅函数占函数向量的连续索引。
P 收集所有声明，再求常量/长度/布局，最后发布 ResolvedProgram；不要求按声明顺序定义函数或 const。
TypeTable.entries 的索引为 TypeId；names 至少包含 `u8`、`u32`、`usize`、`bool`、`()` 与各具名结构。
TypeInfo.layout 全部完成；field_offsets 对应结构字段顺序，其他类型为空；Result 的 payload_offset 必为 Some（无载荷字节时仍保持明确偏移）。
unit 0/1；bool/u8 1/1；u32 4/4；usize/指针 8/8；数组 element.size*length、element.align；结构按字段对齐并尾部补齐；Result 标签/最大载荷按 Core S 第 13 节布局。
P 内部可临时保留布局未完成的节点，但不能发布；递归结构经指针合法，按值/长度/sizeof 布局环拒绝，零数组仍要求元素布局。
type_uses 覆盖源码每个 Type.span，包括局部、模式、cast、Result 构造、sizeof/alignof及嵌套类型出现；相同源码出现保留唯一稳定身份，结果按值长度规范化。
P 还须为每个 ExprKind::Result/PatternKind::Result 的完整 (ok,error) 组合预先 intern Result TypeInfo（组合本身没有 Type.span）；C 按两个已知 TypeId 在具体表中找对应 Result，不自行求布局或新增 Result 型。
repeat_lengths 按 Repeat.length.span 保存已检查的 usize 常量长度；type_uses 和 repeat_lengths 收集不可达语句/defer 的所有出现。
constants 保存具体 TypeId 和已求标量 bits（bool 仅 0/1，unit 为 0）；函数参数名/原始 Type/源码 body 保存供 C 唯一检查，不另生成有类型表达式树。
ResolvedFunction.signature 包含全部参数 TypeId、结果 TypeId、unsafe 位；参数顺序与 parameters 一致。函数名与 const 共用值名字空间，结构在类型名字空间；各空间同域重名拒绝。
函数体不在 P 做正式类型检查；P 为收集其 type_uses/重复长度作语法遍历，不执行表达式，也不决定值/位置、调用或清理行为。

## resolved → typed CFG 的 C 输出保证

C clone P 的具体 TypeTable，只为明确取址生成所需 pointer 类型（model::TypeTable::pointer），不再求长度/布局；这个操作固定为 RV64 8/8。
所有 source Type 从 type_uses 取 TypeId；Repeat 长度只取 repeat_lengths；缺项是 I_INTERNAL 合同错误，不能默默回退至源语义重算。
函数顺序和 FunctionId 保持 P 输出；Function.name 为源码名，symbol 约定 `__core_s_f{FunctionId.0}`，无 ABI/属性修饰。
每个局部/临时为 SlotId，slot 的 TypeId 固定；Parameter_slots 与 signature.parameters 一一对应，每个函数参数有独立逻辑槽。
每条 IR 指令及块保留 Span。Compilation 不包含 AST、HIR、未求类型或布局服务；backend 不接触 ResolvedProgram。
Operand::Immediate 用于 unit/bool/整数（bits 已正确截断且 bool 合法），及已定 pointer 类型的 null 零值；其他指针值及聚合值放槽。
所有 struct/array/Result 即使 size<=8 均为聚合；unit 或任意 size=0 参数省机器位置，但源码实参求值仍由 C 完成。
Set/Store/Load 为标量操作；聚合构造用独立槽+成员写入，聚合读/写及 snapshot 用 typed Copy，src/dst 的类型必须相同。
Copy 的大小来自已完成类型布局，复制填充原始字节不把其当值读；不会暗中 retain/release/初始化填零。
callee 先确定，实参按源码顺序各求一次并立即 snapshot 到独立槽，再生成 Call；Return/? 的返回值同样在清理前保存。
Call 的 arguments 按签名排列，result 为对应结果槽（unit 可 None）；普通递归通过 FunctionId，不靠后端查源名字。
Place 仅 base（slot 或已保存 kernel 指针 Operand）+固定 byte_offset+最终 TypeId；字段 offsets 来自 P。
数组索引由 C 保存 base/index 并发 Bounds，随后以 usize stride 乘法及 Address/PointerOffset 生成地址；Bounds 只出现一次，B 不自动从 Place 推断检查。
PointerOffset 只接受 pointer base 与 usize bytes，结果 pointer 的目标/const 性由 C明确设定；B 执行 checked 地址加法，溢出 Panic，不开放源整数 as 到指针。
Unary.Deref 是 source 位置操作，Binary.And/Or 是 source CFG 短路；C 不把这三者发成普通 Rvalue 操作给 B。
Rvalue::Cast 只用于整数到整数，目标由 Set.dst 的类型确定；Binary 比较的结果为 bool，其他整数操作结果保持操作数类型。
每块有最终 terminator；Jump/Branch 的目标合法，Branch 条件 bool；Return 与签名相同，unit Return 为其 Immediate；Unreachable 不作为静默成功出口。
defer 体在注册点检查一次并生成片段，捕获绑定身份固定；每出口只复制片段并重映射其内部 slot/block，外部捕获保持原绑定。
退出动作先内后外、同块逆注册顺序；break/continue清理到正确深度，?传播保存 Err，panic 不清理；编译器注册状态无需 runtime flag。
StorageLive/Dead 保存局部/临时语义事实，B 不重分配/零填充或重新推导生命期；最终 frame 槽按布局机械分配。

## B 输出和原型执行 ABI

B 只读 Compilation，输出真实 RV64IMA 汇编；所有已发布整数/指针/布局规则固定于 RV64，小端、LP64、16字节栈对齐。
标量按 a0-a7 再按栈传参；沿 LP64 窄标量参数/返回规则：bool/u8先零扩至32位再符号扩至64位，u32从32位符号扩至64位，usize/指针保留64位。表达式与槽内 u32 仍按无符号32位语义，不能把 ABI 扩展当源符号转换。
聚合参数用各自 snapshot 地址占一个机器位置，callee复制入独立参数槽；此普通 ABI 的源范围仍只是本原型支持的类型，不宣称已实现 C 外部接口。
聚合结果第一机器参数为隐藏结果地址，标量/指针结果在 a0，unit无返回机器值；超过八机器位置必须支持栈参数。B不得按聚合大小猜成标量。
Set 的 Rvalue 类型取目标槽，Load/Store/Copy 取 Place 类型；B可验证 IR 结构并以 I_INTERNAL 拒绝损坏模型，不进行源名字/可写性/模式/常量检查。
普通算术/除零/移位边界、Bounds、PointerOffset 溢出失败发同一个不返回 panic 路径；本原型外部钩子为 `__core_s_panic()`，由 I 提供停止夹具。
该无消息测试钩子是原型接口，不声称实现完整 S 的 panic 消息物理 ABI；源 panic/字符串当前 I_UNSUPPORTED。Unreachable 若误执行也应停止，不能返回正常值。
汇编输出不包含 _start、测试断言、外部 runtime 或ELF链接；I负责提供明确入口及执行夹具。无静态数据/重定位支持，本原型 Compilation 因而没有 data 表。

## 原型接受用例及剩余实施

接受/执行须来自真实源码：Header/Buffer 的 sizeof 数组长度与常量前向引用；take(pair,mutate(addr_of_mut(pair))) 保留旧首实参；聚合返回槽独立；循环 break/continue 清理；指针日志上的 defer LIFO；固定Result match观察嵌套?错误传播。
拒绝用例包括 const→sizeof→数组长度循环、直接按值递归、裸字面量比较反推、缺声明类型、非法载荷绑定/遗漏match、const写、未授权raw访问、defer中return/?/循环及不返回的unit函数。
未实施清单必须实际触发 I_UNSUPPORTED，不得通过旧语义服务完成，也不得报告完整 Core S 或旧合规范用例已经通过。
H 验收只核对接口闭合及默认 cargo check；P/C/B 独立验收各自服务，I 再验真实源码汇编/ELF/执行。接口通过不替代运行或规模测量。
规模记录全部新手写代码、直接path复用手写代码、生成表、测试、执行夹具/平台支持，并把被封存0.5单列；不得压行或把必要算法移出统计。
