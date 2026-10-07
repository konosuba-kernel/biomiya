# Core B：有界原型接口及独立验收合同（R 草案）

用户已选择 D 的 B（T1/T2）。此文件仅冻结 P/V 实验的接口、范围和证据要求；不是生产规范，也不是已实现、已缩减或已移除 deferred 的声明。规则的权威文本为同轮 Core B 规则草案；若规则与实验边界不一致，应显式 unsupported，而不是改写规则适应实验。R 独立验收通过之前不得建立原型。B1 保持冻结。

## 1. 实际可复用的 Rust 边界

实验目录 `experiments/core-b` 是独立 package，不修改根 Cargo.toml：

```toml
[package]
name = "core-b-prototype"
version = "0.0.0"
edition = "2024"
rust-version = "1.96"

[dependencies]
biomiya = { path = "../.." }
```

依赖树包含完整旧库，因此旧 checker 会被编译；**实验运行路径不得调用它**。编译进依赖不等于复用其语义。禁止 `biomiya::check`、inference、旧 constants/deferred 的直接调用、间接包装调用或失败 fallback。也不调用 `driver::main_entry`：现有 driver.rs:282 强制进入旧 check。新 main 仅负责 SourceMap、parse、实验 check、lower、RV64 emit 与文件输出。测试中的基线调用须在单独 baseline harness/process 中，与候选入口分离。

| 复用 API | 实际来源与用途 | 限制 |
| --- | --- | --- |
| `syntax::parse(FileId, &str) -> diagnostic::Result<AstFile>` | syntax/mod.rs 公开 parser::parse；完整原 parser | 不复制 lexer/parser、不改语法 |
| `resolve::Resolver::new(&[AstFile])` | resolve.rs:46；公开 declarations、lookup_visible | 只复用声明/名字解析，不把它当类型检查器；局部 BindingId/作用域表由 P 实现 |
| `TypeContext::{new,intern,define,substitute,kind}`、`TypeDef` | types.rs 公开类型 intern、替换和名义定义 | type resolver、签名表、泛型定义点检查仍须由 P 新写并计量；不存在公开的独立旧 type-check API |
| `hir::{Program,Function,Expr,Statement,Pattern}` | hir.rs 全部必要字段公开 | 直接构造；不镜像完整 ExprKind、不携带未求解变量 |
| `LayoutEngine::new(&mut TypeContext).layout(TyId)` | layout.rs:47,54；返回公开 Layout/VariantLayout | 仍复用既有依赖分析和 ConstLen 解释；本实验仅喂入具体、已支持类型 |
| `lower::lower(&hir::Program) -> Result<ir::Compilation>` | lower.rs:25 | 复用原快照/清理/MIR，不重新实现 |
| `target::rv64::emit(&mut compilation.types, &compilation.module)` | target/rv64.rs:29；Emission.assembly 公开 | 直接实际 RV64；汇编/ELF 可走现有 toolchain 公共 API或独立命令，不走旧 driver 源检查 |
| `ir::{Data,Relocation}`、`diagnostic::{Diagnostic,Result}` | ir.rs:235,242；diagnostic.rs:18,43 | serializer 输出真实 Data；错误带原 Span |

上述可编译性依据是实际签名、导出和字段可见性检查；当前 rustc 为 1.96.0，满足 manifest。R 不伪称已执行候选 cargo check：实验尚不存在，完整构造不变量只能由 P/V 编译、lower/runtime 验证。生产 helper 的私有可见性不可用修改核心源文件的方式绕过；需要的有界 helper 在实验内新增并计数。

## 2. 文件所有权与无需互等的接口

P 唯一写 `Cargo.toml`、`Cargo.lock`（若产生）、`src/lib.rs`、`src/main.rs`、`src/check.rs`（或 `src/check/**`）、`tests/check.rs`、`tests/check/**`。V 唯一写 `src/constant.rs`（或 `src/constant/**`）、`tests/constant.rs`、`tests/constant/**`。fixture 跟随各自 tests 子目录。P/V 不改 src 核心、文档、对方文件；公共类型不另开一个共享修改文件。每方证据写各自 implementation/P 或 V 目录。E 拥有差分 harness 和最终合并证据。

P 的 lib 初始使用 feature 隔离 V 未落地模块，使 P 能单独编译；V 独立构造真实 biomiya HIR，不调用 P check。P 在 manifest 预留 `constant = []`，在 lib 预留以下内容（R 通过后由 P 最先提交这三个骨架，不要求 V 已实现）：

```rust
pub mod check;
#[cfg(feature = "constant")]
pub mod constant;
```

V 在 P 骨架到位后独立编译，不需等 P 类型检查实现。V 测试文件首行 `#![cfg(feature = "constant")]`。默认 CLI 支持 `check FILE`、`emit FILE -o FILE.s`，只走 P；V 不要求 P 增加静态初始化前端。P/V 组合测试属于 E，避免 P 为等待 V 偷接旧常量执行器。

若 P 的 check 模块尚未编译，V 还可拥有 `build/simplification/implementation/V/harness/` 的临时独立 Cargo package，仅 path 依赖仓库 biomiya，以绝对 `#[path = "/home/mii/code/konOSuba/biomiya/experiments/core-b/src/constant.rs"] pub mod constant;` 引用同一 V 源文件，integration test 引用该 harness 的 constant 模块。运行 `cargo test --manifest-path build/simplification/implementation/V/harness/Cargo.toml`；测试数据和测试源码复用 V 文件，不复制解释器。harness manifest/helper 与测试计量单列；不能把这个编译当作最终整 crate 集成通过。这样 V 不依赖 P 骨架可编译性，也不并发修改 P 文件。

P 对外接口固定如下，内部实现组织自由：

```rust
pub fn check(files: &[biomiya::syntax::AstFile])
    -> biomiya::diagnostic::Result<biomiya::hir::Program>;
```

V 对外类型和函数全部定义并拥有在 constant 模块内：

```rust
use biomiya::{diagnostic::Result, hir, ir, source::Span,
              types::{TyId, TypeContext}};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstValue { pub ty: TyId, pub kind: ConstKind }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wrapper { MaybeUninit, UnsafeCell, Atomic }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstKind {
    Unit,
    Scalar(u64), // bool/int 位模式；宽度及有符号性从 ty 查询
    Aggregate { variant: Option<usize>, fields: Vec<(usize, ConstValue)> },
    Relocation { symbol: String, addend: i64 },
    Uninit,
    Wrapped { wrapper: Wrapper, value: Box<ConstValue> },
}
#[derive(Clone, Debug)]
pub struct ConstInput {
    pub locals: BTreeMap<hir::LocalId, TyId>,
    pub symbols: BTreeMap<String, TyId>, // 允许引用的声明类型；不是内存值表
}
#[derive(Clone, Debug)]
pub struct DataSpec {
    pub symbol: String,
    pub writable: bool,
    pub exported: bool,
    pub section: Option<String>,
    pub span: Span,
}
pub fn validate(types: &TypeContext, expr: &hir::Expr, input: &ConstInput)
    -> Result<()>;
pub fn evaluate(types: &mut TypeContext, expr: &hir::Expr, input: &ConstInput)
    -> Result<ConstValue>;
pub fn serialize(types: &mut TypeContext, value: &ConstValue, spec: &DataSpec)
    -> Result<ir::Data>;
```

这些定义所需的 LocalId 已实现 Ord，TyId 已实现 Eq，HIR Expr/Span 和 TypeContext 均公开。V 验收要求从 integration test 直接 intern Int/Tuple/Option、define 一个 Enum、构造 Expr/Block/Pattern 再 evaluate，不是拼一个私有玩具 IR。ConstValue 不含 Expr，不存 AST，也不把已经归约的 HIR 当结果。serialize 可递归查询真实 Layout，但不得回调 evaluate，不认识 HIR。evaluate 必须先 validate，防止调用者漏掉静态全遍历；内部 exec 只处理选中路径。

## 3. P：直接定型的最小可证伪范围

源输入采用已有 parser。支持单文件普通函数、显式参数/返回类型、普通及有限泛型函数声明、基础类型、tuple 和内建 Option 构造。P 的 expected enum 构造最小证据可限于真实 `TypeKind::Option`，不要求初步实现 Result/全部 nominal 前端；V 的独立 nominal enum HIR 测试不证明 P 已能检查它。支持整数/布尔/unit、局部单名 let（含注解）、tuple 构造/字段、整数运算/比较、block、if/else、普通调用、完整显式类型实参泛型调用及已具体 expected enum 构造。不得把所有构造者等同普通函数。无泛型推导变量、peer 求解、Span hint 回灌；唯一 `check_expr(node, expected: Option<TyId>)` 规则入口。AST 遍历允许 type resolution、名字绑定、泛型依赖审查，不允许再做一套表达式推导分类。

具体预期进入 let 注解、已确定函数参数/返回、tuple 对应元素及 if 两臂。无预期的整数立即 i32；左操作数不能从右操作数学类型，if 另一臂不能反馈已检查分支，普通泛型函数的缺省类型实参不能从参数或返回预期补齐。若无预期，分支分别检查后要求规则规定的确定关系。构造者省略仅接受具体预期唯一确定同一枚举实例；None 等无载荷也必须由预期或显式实例确定。类型实参不能局部缺省、不能用 `_` 偷造变量。精确相等、允许的 expected adaptation、core_compatible 是不同语义；不把后者当所有表达式关系。未涵盖的 reference/effect adaptation 必须 unsupported，不能误接受。

泛型定义和具体实例调用同一检查入口：定义点可保留 TypeKind::Param 作为已声明的符号类型，不是推导变量；与参数无关的错误即使未调用也必须报告。原型允许有限 identity/forward/tuple 泛型体，参数相关运算/投影若无能力规则便 unsupported。具体实例 HIR 不得遗留 Param。实例表采用明确 key 与 Active/Ready，递归实例及不同类型参数增长须有明确诊断或资源上界，不能无限展开。新增名字/实例来源 helper 全计入 P。

检查所有声明体，包括未使用普通函数、未实例化泛型函数，不能只走 reachable 函数。模板体只要含未支持的语法/参数能力，该**整个程序**返回 `E_CORE_B_UNSUPPORTED`，不能静默接受未调用声明。有限 Param 策略若实现不了定义点检查，则对应泛型程序全部 unsupported，且显式泛型原型验收未通过；不能以实例化后重检代替定义点门禁。

原型至少正反测试：

| 条件 | 必须观察的结果 |
| --- | --- |
| `let x = 1;` 后在 u64 场合使用 | x 已 i32；不能回写 u64；注解 u64 的对应版本通过 |
| 无预期 `1 + explicit_identity::<u64>(2_u64)` | 按 B 拒绝；显式 `1_u64` 对应版本通过 |
| 普通/显式泛型调用 | 参数按已确定签名检查；省略/部分实参，即使返回预期完整，也拒绝 |
| tuple 字段后调用，if 分支交换顺序 | 一样的确定类型和结果，不依赖 Span 缓存/检查次序 |
| `let p: Option<u32> = Some(3)`；无预期 Some；预期不匹配构造者 | 首个通过，后者要求显式实例或拒绝；显式实例通过 |
| 未调用泛型体内明确类型错误 | 定义时拒绝；不能只检查被用实例 |
| 无限递归实例/资源增长 | 有界停止并诊断，不能 stack overflow |

所有接受 case 都产出真实 Program，并至少对无泛型、泛型显式调用、tuple/field、if 的可执行 fixture 完成 lower→RV64→实际 RV64 运行。HIR 必须保留 scope/local/span/definition/signature/type_arguments；有 Resolver 时放入 Program.declarations，不伪造全量 Extend 达标。scope 与 LocalId 数组索引一致。省略方法、借用/移动完整规则、defer/?/match/asm/unsafe/address spaces/C ABI/static 等广泛功能须返回 `E_CORE_B_UNSUPPORTED` 和具体节点/Span；未实现不能报告为语言拒绝，不能作为 T1/T2 变更证据。

## 4. V：静态允许与按路径执行分离

V 不重写 parser、不调用旧 checker。输入合同是受信的、已经定型且满足 HIR 类型/作用域不变量的 HIR；类型合法性权威属于 P，或独立基线 harness 中的旧 checker。测试直接构造满足这些不变量的真实 typed HIR；P 能支持的源场景由 E 另行组合。validate 只负责 const 允许性及静态规则，所有分支包括死分支、所有未使用 initializer 都遍历，不重新推导字段、操作数或局部类型。合法未选择分支中的除零/溢出不执行；未选择分支里的 Call/Asm/不合法 relocation 或未知 intrinsic 仍拒绝。执行顺序遵循 HIR fields 原书写顺序，布局按 field index 定位，不排序后执行。

最低支持：Unit/Integer（含 bool 的类型约束）、Local、基础 checked unary/binary（至少加减乘及比较、布尔短路）、Aggregate 的 tuple/Option/Result/nominal enum、Field、Block 的 Bind/Wildcard/Expr/tail、If、具体 `__const.sizeof` / `__const.alignof`。const block 在既有 HIR 用 Block 表示，无需另造 AST；V 局部环境存 ConstValue，不存 Expr。整数宽度/带符号溢出由 type 检查，serializer 以 RV64 小端输出。一般 Call、Match、循环、assignment、defer、return、arrays/string/slice、panic 等本轮 unsupported，不能让死分支遮蔽。

重定位至少支持 Function、RawAddress(Global) 及稳定符号+带符号 addend 值；若源 ptr_add 尚未实现，用直接 ConstValue relocation 测 serializer，但另需真实 HIR 的 RawAddress→evaluate→relocation 证据。input.symbols 是可信声明表；validate 只检查符号是否具备常量地址资格，不能读取 Global 的值，不重复指针/function 类型规则。Data 的 relocation.offset 是对象内字节位置，symbol/addend 保持独立，聚合内偏移来自 Layout；地址不变成宿主指针或假数值。链接验证给该符号真实 target 定义，检查最终位置/字节或运行返回结果，不仅 Debug 字符串。

Uninit 至少支持 `MaybeUninit<T>`，包括聚合内字段；不接受任意 int/enum 的 Uninit。Uninit 序列化保留不读取的语义，静态镜像可用零填充占位，不能声称值已初始化。V 支持 `atomic.new`、`UnsafeCell.new` 一参数包装，其值由 Wrapped 表示，ty 必须准确匹配 Atomic/UnsafeCell 内层；既有 Core 没有 MaybeUninit::new，因此不接受伪造的 MaybeUninit.new HIR。手工 ConstValue 的 Wrapped::MaybeUninit 可测试表示与序列化，但这不是源语言构造能力；从 HIR 创建已初始化 MaybeUninit、write/assume_init 本轮不支持。serializer 验证包装对应 type，并使用真实包装 Layout；禁止原子读写、Uninit 读取/运算/分支条件及 MaybeUninit assume_init。复杂嵌套数组/字符串包装本轮 unsupported；这只是合法包装初始化的有界覆盖，不是完整保留能力验证。

serialize 输入是 evaluate 产生或测试按同一契约构造的合法 ConstValue；保留简单字段索引、relocation 宽度与对象边界断言/错误，不建设通用不可信 IR schema/type validator。设置例如 64 MiB 图像上限及简单 eval 燃料并测试超限诊断，避免无界求值/分配。padding 字节的比较不作为聚合相等。

V 至少测试：typed tuple/enum 的 payload offset、嵌套 field 的序列化；Block 局部及未使用 initializer；死分支非法操作被 validate 拒绝而合法死分支除零不执行；短路；具体 layout query；Function/global relocation 与聚合内 relocation；Uninit 合法/非法常量用法及包装；图像/求值上限和 serializer 边界错误。输出同功能的真实 Data/relocation 并由 RV64 emitter 消费至少一个镜像。

## 5. T2 的验证边界

P 若不支持数组长度/重复/static_assert，必须逐项 unsupported，**不能声称 T2 已实现**。独立规则 fixtures 可以记录应拒绝的 generic length；其端到端候选结果在该阶段是 unsupported，不是合格的语义拒绝。V 仅求具体布局，并复用 LayoutEngine 原来的 validate_dependencies、ArrayExpr/ConstLen 缓存机制；单次具体 sizeof 成功不证明消除了 deferred。

泛型按值布局、别名/常量块/死分支的间接泛型依赖、const↔layout 循环、指针截断表示依赖、未实例化模板的完整依赖检测，是后续扩大原型必须完成的门禁。若最小阶段加入测试，它们的覆盖与结果单列；未实现项记 `unverified`，不得变成通过。不建立保存整个旧 Context 的候选延迟入口。既有布局循环测试只能说明旧复用层行为，不算新依赖架构证据。

## 6. 命令及独立接受条件

R 通过后的命令合同：

```sh
cargo check --manifest-path experiments/core-b/Cargo.toml
cargo test --manifest-path experiments/core-b/Cargo.toml --test check
cargo test --manifest-path experiments/core-b/Cargo.toml --features constant --test constant
cargo run --manifest-path experiments/core-b/Cargo.toml -- check experiments/core-b/tests/check/fixtures/shared.vt
cargo run --manifest-path experiments/core-b/Cargo.toml -- emit experiments/core-b/tests/check/fixtures/shared.vt -o build/simplification/implementation/P/shared.s
build/simplification/baseline/biomiya build --emit asm -o build/simplification/implementation/P/baseline-shared.s experiments/core-b/tests/check/fixtures/shared.vt
```

E 使用与冻结基线相同 rv64 工具链/platform/runtime runner，记录命令、版本、exit status、stdout 和输入/输出 hash；执行 harness 不得为候选换成宿主解释器。若平台不能运行，记录 blocked/unverified runtime，不以汇编产生冒充执行成功。测试中真实 lower 与 emit 是必选，具体 RV64 runtime 工具发现后记录可复现的完整运行命令，不编造不存在的 runner。

P 独立接受至少要求一个端到端 QEMU 可执行例子，源程序同时包含局部、tuple field、if、普通与显式泛型调用并返回可断言结果；其余有界 fixture 可沿用同一运行 harness。现有工具链/runtime 可以复用，但完整清单及成本透明记录；QEMU 工具不可用时该运行验收项未通过。一个 Option 或单个 nominal case 不构成全量泛型证明。V 是有界 HIR 执行器，旧 layout 的 ConstLen 解释仍存在，不能称全编译器常量规则已经统一。

P、V 分别由未参与实现的人/agent 独立接受：读源确认无旧 check 路径，cargo 与 fixture 运行，检查 Program/ConstValue/Data 不变量，对单入口及静态/动态遍历作反例。R 公共接口可见性检查不替代此项。E 再组合候选 HIR 与 V，验证同类型同布局、真实 MIR/运行结果。原型通过不允许发布完整新版编译器；生产门禁及 Extend 全量覆盖属于 I。

## 7. 可比基线与透明计量

fixture 分四类：shared（同一源、同一功能、双方支持）、changed（B 有意拒绝的旧推导及显式迁移后同功能源码）、unsupported（候选边界）、known-baseline-bug（按规范独立判断，如 Copy 字段显式 deref 的旧误拒）。不能把 changed/unsupported 汇总为兼容失败或通过，不能把 old binary 当规范。shared 至少上述整数/tuple/if/显式泛型；V 等效 static 源使用冻结 parser/check，候选用真实手建 typed HIR，报告 **V 前端未覆盖**，同操作/ty/layout/值语义可比，不把建 HIR 的测试 LOC 与用户程序成本混合。

双方记录 acceptance、诊断类别与 Span、规范化类型快照（按类型显示而非不同 intern 编号）、MIR events、运行输出/exit、静态字节及 relocation。changed 保留原源与显式迁移源，记录用户物理 LOC/token/类型注解/实参数量和 ABI/layout 是否相同；固定容量迁移不得默认等价。

计量账本至少列出：新实验手写代码；实验测试/工具/文档；复用旧模块清单和完整手写成本；有界同功能旧函数毛覆盖；真正删除的旧源码；所有新增 helper/manifest/generator；格式化后全编译器候选净差。path 依赖旧 compiler 未删除任何行，最小实验必然增加仓库代码；可比较子集实现成本但不得声称全树净减。旧 inference/deferred 行数和受影响 module 行数不是删除量。新规则若挪入 helper 则继续计入；复用 layout 的旧 Len/dependency 成本不能隐藏。

理解成本由独立 reviewer 对双方做同样维护任务：增加整数/const 操作；解释立即默认与实例规则；追踪具体布局及指出未验证泛型依赖；定位一个失败出口与 lower 快照/清理的复用；检查 provenance/Extend 边界。记录实际打开文件、规则权威位置、改动点和跨阶段状态，而非综合评分。原型不支持的清理或布局任务据实标记未完成，不借复用层冒充新架构改进。
