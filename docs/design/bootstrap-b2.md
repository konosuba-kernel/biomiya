# B2 输入、运行库与第一批自写源码

2026-10-10，接口合同 H r1。接续 B0/B1，不声称完整 R1a 或自编译。本阶段必须让目标语言程序在 RV64 Linux 中读取 manifest、逐个读取并扫描源文件、写出按顺序连接的源字节；错误和短 I/O 有真实执行证据。

## 语言增量 F

保留既有 pilot 接受行为，新增 R1a 中实际需要的能力：

- 字符串字面量为 `[]const u8`，支持 R1a 转义，静态存储，无隐式零结尾。空字符串仍有非零地址。
- `[]T`／`[]const T`，16/8 布局；`.data`／`.len` 只读。元素访问需要 unsafe，长度检查使用动态长度。复制描述符不复制目标。
- 接受 `*T`／`*const T`，旧 `*kernel` 写法作为原型兼容形式暂留。
- `core::addr_of[_mut]`、`addr`、`ptr::<T>`、`ptr_const`、`ptr_add`、`ptr_add_bytes`、`slice_of[_mut]`、`slice_from_raw_parts[_mut]`、`slice_const`。按 R1a 类型／unsafe／权限约定；不隐式数组转切片。
- `static`／`static mut`：本阶段初始化器覆盖标量字面量／具名常量、数组列表／重复、结构构造、字符串和静态根取址；其他合法 R1a 初始化器明确 I_UNSUPPORTED。全零可写对象进入 BSS。可写 static 读取／写入要求 unsafe；仅根取址不读值。
- `extern abi(C) fn name(...) -> T;`，仅现有标量／指针签名，unit 可作结果；调用须 unsafe，不生成函数体，也不隐式链接 libc。聚合 C 参数拒绝。
- 多文件 `module a::b;`、模块 `import a::b;`、顶层 `pub`；路径从当前模块或导入起，`root::` 从根起。默认私有，跨模块私有名字拒绝，重复及导入冲突拒绝；模块文件合并。首版跨模块公开结构要求所有字段显式 pub，混合公开类型／私有字段形态明确 I_UNSUPPORTED。公开签名不能泄露私有类型。不是完整 R1a 名字系统迁移。

不加入新整数宽度、通用 CTFE、泛型或优化。Linux 负 errno 在 usize 中按原始位模式传递，由运行库识别高位错误区间；这不冒充已实现 isize。旧 pilot 比较定型规则及常量布局规则继续沿用并在示例中使用明确字面量后缀。

### 冻结的跨节点 Rust 数据接口

F 拥有 ast.rs/model.rs 的实际修改，须先发布这些记录再实现消费者；M 不自行改共享类型。现有成员保留，新增：

```text
ast::Program { module:String, imports:Vec<(String,Span)>, declarations:... }
ast::Declaration { public:bool, kind:..., span:... }
ast::Field { public:bool, ... }
ast::DeclarationKind::Static { name:String, ty:Type, mutable:bool, value:Expr }
ast::DeclarationKind::ExternFunction { function:Function, symbol:String }
ast::TypeKind::Slice { element:Box<Type>, const_:bool }
ast::ExprKind::String(Vec<u8>)
ast::ExprKind::Pointer { target:Type, address:Box<Expr> }

model::TypeKind::Slice { element:TypeId, const_:bool }
model::ResolvedGlobal { name:String, ty:TypeId, mutable:bool, initializer:ast::Expr, span:Span }
model::ResolvedProgram { globals:Vec<ResolvedGlobal>, externs:BTreeMap<FunctionId,String>, ... }
model::Compilation { data:Vec<machine::DataObject>, externs:BTreeMap<FunctionId,String>, ... }
model::Rvalue::SymbolAddress { symbol:String }
model::Rvalue::Bitcast { value:Operand }  // pointer<->usize or pointer const view, never arbitrary aggregate
model::InstructionKind::BoundsDynamic { index:Operand, length:Operand }
```

外部函数仍占 FunctionId 和函数向量的一项；checker 产出有签名但空 blocks/slots/parameter_slots 的 Function，symbol 仍为 `__core_s_f{id}`。backend 按 Compilation.externs 跳过正文验证／发射并把调用指向外部 ASCII symbol；仍验证 C 签名和类型编号。所有外部定义必须在本次 .s0 输入中解决。

F 提供 `frontend::resolve_files(Vec<ast::Program>)->Result<ResolvedProgram>`；原 resolve(program) 调用它处理单文件。模块合并与路径消解可放 modules.rs。合并后内部名字为确定性的限定字符串，机器符号仍使用编号。core 完整路径不可被用户声明夺走；旧裸内建兼容行为保留。

M 拥有 machine.rs 的数据扩展，字段名固定如下：

```text
machine::DataRelocation { offset:u64, target:String, addend:i64 }
machine::DataObject { symbol:String, mutable:bool, align:u64,
                      bytes:Vec<u8>, zero_fill:u64, relocations:Vec<DataRelocation> }
machine::Program { instructions:Vec<Inst>, data:Vec<DataObject> }
```

bytes 非空时 zero_fill=0；BSS 为 mutable、bytes 为空、zero_fill>0。零大小对象仍分配一个对齐哨兵字节，逻辑大小由类型决定。字符串池命名 `__core_s_string{index}`，全局按声明顺序 `__core_s_g{index}`；根地址和字符串描述符通过 ABS64 重定位。F 的所有编译产物数据放 Compilation.data，后端完整复制到 Program.data。

## 机器与映像 M

保留 B0/B1 API；编码器另提供 `encode_with_symbols(&Program,base:u64,&BTreeMap<String,u64>)` 和 `text_size(&Program)->Result<u64>`。外部数据符号只供 Address 使用，Branch/Jump 仍须指向本程序 text 标签。文本和数据不能重名。image 分配 text/R/RW/BSS 后回填 PCREL 和 ABS64；逐项检查对齐、范围、加数溢出及文件／内存长度。总映像跨度不超过 64 MiB。R/RW 空段保留占位字节，BSS 不写文件。assembly 调试路径也输出相同数据／重定位语义供工具链对照。

`s0::parse(FileId,&str)->Result<machine::Program>` 处理固定子集：标签、x0..x31、立即数，现有 machine R/I/load/store/branch/jalr/ecall/unimp，以及 address/call/jump 三个伪指令；不得通过外部汇编器实现。load/store 使用 `rd, offset(base)`，jalr 同型；register-register 与立即数形式分别对应现有枚举。小写十进制和 0x 十六进制，可带负号。整行／尾部 # 注释；一行一条指令，标签独占一行。无 li/la、宏、include、任意表达式、数据伪指令、自动松弛；本阶段未提供的 CSR/A 指令明确 I_UNSUPPORTED。

以 `.L` 开头的标签按 FileId 私有化，其余标签是显式 ASCII 外部名字。出错有文件和行对应的 Span。纯数据放目标语言 static；不是 GNU 汇编兼容层。

## 清单 A 与系统入口

新增 `core-s project MANIFEST [--root DIRECTORY]`。manifest 延续 bootstrap-v1 的行格式：format/profile/entry/source/assembly/output；format=1，profile=linux-user，其余 profile I_UNSUPPORTED。format/profile/entry/output 恰好一次，输入至少一个 source，未知项、重复输入、空参数、多参数拒绝。路径相对 --root（默认当前目录），不相对 manifest；拒绝绝对、空分量、`.`、`..`、反斜线与空白。限制文件路径 96 UTF-8 字节、source 32 个、assembly 8 个、manifest 16 KiB。

A 提供独立 std-only `project::parse(&str)->Result<Manifest,String>`，Manifest 包含 `entry:String, sources:Vec<String>, assemblies:Vec<String>, output:String`；配置错报告带行号字符串。I 负责读文件、SourceMap、多文件编译、拼接 s0、原子发布；输入源或汇编与输出 inode 相同必须拒绝。既有 check/build/asm 单文件接口保留。

Linux `_start` 将初始 sp 保存于 callee-saved x27/s11，再调用选定的无参数 u32 入口；所有普通函数和 .s0 C ABI 例程保留 x27。runtime/linux.s0 实现 `__b2_syscall6(n,a,b,c,d,e,f)->usize`、`__b2_argc()->usize`、`__b2_argv(index)->*const u8`。后者由运行库先核对 argc，读取 Linux 初始栈上的 argv；不声称 Linux 提供 SOS BootArgs。

## 目标语言源码 R

runtime/*.vt 实现字节视图、错误结果、argv、open/read/write/close 及有界缓冲／分配；Linux 数字仅在适配模块／汇编中出现，不散落在 scanner/manifest 代码里。短 I/O 循环检查零进展、EINTR 和错误；失败关闭已取得的文件。测试用确定性短 I/O 适配替换 syscall/provider 符号，不能由宿主替被测程序复制文件。

compiler/source.vt 提供具体 Source 和游标；compiler/scan.vt 给出带字节范围的 Token，覆盖本阶段 ASCII 标识符、数字、字符串、空白、标点、行和嵌套块注释，错误定位及 EOF。非 ASCII 标识符明确报告未支持，不冒充完整 Unicode 词法器。compiler/manifest.vt 解析实际清单，app/copy.vt 用这些模块读取清单、扫描并连接所有 source 的原始字节，写一个新输出文件；已有输出拒绝，失败清理本次新建输出。零字节源、跨缓冲边界、多个文件、非法清单、缺失输入及短 I/O 均有验收。

## DAG 与验收

H（主代理架构）→ F（语言）／M（机器数据和 .s0）／A（主代理清单解析）；F+M+A → R（主代理运行库和自写源码）；F+M+A+R → I（主代理 CLI 与组合交付）。独立验收者不修改产品代码，逐节点核对候选摘要、反例和实际命令；当前并行写入互斥。

F：ast/model/frontend/check/modules 及相应 tests；modules.rs 可作为 frontend 的私有子模块，不改 lib.rs。M：machine/backend/encode/image/s0 及 backend/image/s0 tests；A：project.rs 及其内置测试。H 预先加入 gated s0/project 模块声明；主代理在 I 接入 pipeline/main 与 tests/b2.rs，不静默替换 F/M 接口。R 自带 tests/b2_runtime.rs 库级构建／执行夹具，直接组合 F/M API 验证目标源码，不依赖尚未完成的 CLI，因此没有 R/I 环。

原 B0/B1 判定保留为历史版本。B2 对新候选重新验收，能力新增使旧“不支持”用例失效时，用仍未实现的真实能力替代该诊断类别测试，并在报告中列明，不能降低既有正确执行／错误拒绝要求。

必须交付：真实 RISC-V 程序完成读清单／多源读取／扫描／按字节写出；直接映像对照外部工具链；静态可写／只读和 BSS、ABS64、动态切片检查、外部符号缺失、权限／模块错误的测试；短 I/O 及故障处理；无外部工具的生产构建；清洁重建；当前手写编译器、目标运行库／扫描器、生成表和测试分别计数。原型 B2 通过仍不代表 B3 自编译。
