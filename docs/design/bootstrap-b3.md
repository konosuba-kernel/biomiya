# B3：目标语言编译器与 Linux 自编译固定点

2026-10-11，H r3 **候选，尚待独立验收**。本文件细化 bootstrap-v1 的 B3，不取消 B4–B6；Linux 自编译不是 SOS 内自举。B2 源码、命令和测试继续保留。

## 1 交付与可证伪条件

生产路径是：宿主 Rust C0 只编译一次目标语言编译器源码，得到 RV64 ELF C1；C1 读取相同 manifest／VT／s0 字节生成 C2；C2 再生成 C3。C2 与 C3 的完整文件逐字节相等，且 C1、C2、C3 都实际编译并运行独立行为程序。C1/C2 运行环境不给 Rust、Clang、LLD、Python 或其他宿主编译服务。

编译器不得读取自身可执行文件、复制预制 ELF、将输入摘要映射为预制输出、嵌入宿主编译器产物模板来假造翻译，亦不得通过宿主 RPC 代编译。验证将修改独立函数的运算、控制流和静态字节，再观察新产物行为改变；修改编译器源码中一处可观察诊断也必须由 C1 重新编译并改变 C2 行为。

B3 仍是明确的 Core S 原型：不新增语言机制来迁就编译器写法。ASCII 标识符、现有 u8/u32/usize、固定 Linux profile 和资源配额是实现边界，不冒充完整 R1a。合法但未实现的语言形态返回 `I_UNSUPPORTED`；类型错误仍返回 `E_TYPE`，名字错误 `E_NAME`，语法错误 `E_PARSE`，资源超限 `I_LIMIT`。任何尚未完成的下列承诺都是未通过项，不以缩减测试代替。

## 2 语言和输入范围

保留 B2 的主要可执行能力：

- module/import/pub、多文件同模块合并、私有名字拒绝和公开签名可见性；公开结构全部字段 pub 的 B2 限制不变。
- bool/unit/u8/u32/usize、原始指针、结构、固定数组、切片、Result，明确类型局部变量；结构与聚合按值快照语义。
- 标量常量、static/static mut、B2 静态初始化器、字符串、sizeof/alignof、现有 core 指针／切片内建。
- 函数、标量 C extern、按序实参求值、聚合内部 ABI；超过八个机器参数走栈。
- if/else/while/loop、break/continue、return、unsafe、match 的 B2 模式、defer、Result 构造和 `?`。
- 现有整数运算：checked 和 wrapping 分开；除零、窄整数溢出、移位范围、数组／切片越界触发明确 panic。沿用 B2 字面量定型，不暗改为完整 R1a 双向推导。
- `.s0` 的 B2 子集及同样的立即数、标签和寄存器检查；不新增 GNU 汇编兼容层。

未提供的枚举、类型别名、用户泛型、闭包、间接函数值、CTFE、Unicode 标识符、新整数宽度、CSR/A 汇编和其他 profile 明确 `I_UNSUPPORTED`。UTF-8 字符串内容可以作为字节保存；标识符限制不能误拒绝注释和字符串中的 UTF-8 内容。

命令为 `bmc build MANIFEST` 和 `bmc check MANIFEST`。manifest 与 B2 相同，路径相对启动工作目录；build 输出为 manifest 的 output。第一版输出遵循 B2 的新建文件规则，已有文件拒绝；构建失败不得留下本次不完整输出，不得截断输入。bootstrap 测试用三个不同 output 路径但其他输入字节相同，output 路径不进入产物。仅 check 不创建 ELF。

## 3 源码所有权与 DAG

新目标代码放 `core-s/bootstrap/`，模块 `bootstrap::*`。已有 B2 `compiler/` 和 `runtime/` 保留；必要的适配增量必须另行列明而不是默改旧验收结论。

| 节点 | 作者及文件 | 依赖 | 独立验收 |
| --- | --- | --- | --- |
| H | 前端作者：本合同、model.vt 与模型探针；主代理协调接口 | 已验收 B2 | 范围、表、ABI、错误和 DAG 无环 |
| L | 主代理：lex.vt、输入/清单/IO 适配及其测试 | H | 词法、输入快照、短 I/O、错误位置 |
| F | 前端作者：parse.vt、types.vt、check.vt、ir.vt 及前端夹具 | H、L | 从源码产生合法 typed IR；接受/拒绝/快照/清理反例 |
| M | 后端作者：rv64.vt、image.vt、s0.vt 及后端夹具 | H | 手造合法/非法 IR、机器字节/ELF 与独立工具链对照 |
| I | 主代理：main.vt、manifest、端到端测试、README、度量 | F、M、L | C1→C2→C3、扰动、清洁构建、完整回归 |

H 实際 model 接口验收后，L/M 正式并行；L 验收后 F 正式执行。F 与 M 不互相调用：驱动逐函数调用 F，再调用 M，复用函数 arena。独立验收者不得修改被验收的产品文件。model 的 fail 使用 B2 已有 syscall 适配，不依赖 L。

## 4 model.vt：具体表，不引入容器框架

所有跨模块记录字段 `pub`。记录使用 u32 的 tag/id 和 usize 的字节数/位模式；不依赖布局 memcpy 到文件。表 ID 0 是缺省/无值，合法实体从 1 开始（基本类型也如此）。第一版边界接口暂用 u32 ID，以避免大量包装结构转换；每张表的 getter 和所有消费者必须检查对应表界限。这是与 bootstrap-v1 单字段 ID 包装提案的**显式实现差异**，不是改变语言类型身份。

每张表使用具体的 `pub static mut` 定长数组和长度；数组元素不搬迁。写入前检查配额，消费者只保存 ID。静态内存/启动缓冲同样计入峰值，不因位于 BSS 而漏账。源码和全局表在一次构建期间保留；Slot/Inst/Arg/AST/Binding/清理表在每个函数后重置。不得在符号/回填中保存函数 arena 指针。

以下字段顺序为共享 API，record 中所有未说明字段为 u32；`usize`、切片、bool 显式标出。

```text
Span   { file, start, end }                         // 字节偏移，源总量限制保证 u32
Source { path:[]const u8, bytes:[]const u8, module_id, import_first, import_count }
Token  { kind, file, start, end, number:usize, suffix }   // 当前 Source 内位置
Name   { bytes:[]const u8, hash:usize }             // stable owned/source bytes
Import { file, module_id, alias, span:Span }
Type   { kind, a, b, flags, size:usize, align:usize, first, count }
Field  { name, ty, flags, offset:usize, span:Span }
Decl   { kind, name, module_id, file, flags, ty, first, count,
         start, end, init_start, init_end, symbol, bits:usize, state }
Param  { name, ty, span:Span }
Slot   { ty, flags, offset:usize, span:Span }
Inst   { op, dst, a, b, c, aux, bits:usize, span:Span }
Data   { symbol, region, align:usize, size:usize, first:usize, count:usize }
Symbol { name, region, offset:usize, size:usize, align:usize, defined, kind }
Fixup  { region, kind, offset:usize, symbol, addend:usize }
DataRelocation { object, offset:usize, target, addend:usize }
```

`Source`/`Name` 的切片存储在稳定源码或 Name 字节池；不得指向已退出函数栈。模块和限定名字是 intern 后 NameId，根模块采用空字节名。Name ID 稳定性只由插入顺序决定。H 初始 name 查找使用 hash 预筛选加线性扫描，尚未引入哈希槽；这是 v1 开放寻址提案的实现差异，后续只可在测量后改变。固定文件顺序、声明顺序、函数顺序和字符串首次遇到顺序决定编号。

Type kind：1 Unit、2 Bool、3 U8、4 U32、5 Usize、6 Pointer、7 Array、8 Slice、9 Struct、10 Result。基本 TypeId 与 kind 相同，`types[1..5]` 固定。Pointer/Slice 的 a=元素 TypeId，flags bit0=const；Array a=元素、b=长度；Struct first/count=Field 范围（名义身份为自身 TypeId）；Result a=Ok、b=Err，first=payload 字节偏移。size/align 只在完整布局后发布；结构/Result 依赖通过 DFS state 拒绝无穷值类型，指针可以指向未完成名义类型。

Decl kind：1 struct、2 function、3 extern、4 const、5 static。flags bit0=pub、bit1=unsafe、bit2=mutable。name 为模块内短名字；module_id 为模块 NameId。ty 是结构自身/函数结果/常量静态类型。function/extern 的 first/count 指向 Param；函数 start/end 是函数正文 token 字节区间；static/const 的 init_start/end 同理。extern symbol 是 SymbolId，其 Symbol.name 指向显式 ASCII linker NameId；普通函数/静态的 symbol 指向分配的 SymbolId。state：0 已登记，1 正在布局/常量求值，2 完成；循环依赖要拒绝。

Data region：2 rodata、3 data、4 bss；first/count 是 initialized 字节池范围，bss count=0。size 是逻辑对象占用，零大小对象实际保留一字节；static 类型大小仍为零。字符串对象没有自动 NUL；空字符串保留非零地址。Symbol region 1 text/2 rodata/3 data/4 bss，kind 0 未指定/1 function/2 data/3 local label。model::symbol 的 kind=0 引用可匹配任意已存在种类；后续非零声明将原0升级，不允许两个不同非零种类冲突。Fixup kind 1 PCREL_PAIR、2 ABS64，addend 为补码位模式；写回前做数学范围验证，不截断。

配额初值（均可在本地常量处调整，超限仍需 I_LIMIT）：source 32 项共 4 MiB；assembly 8 项共 1 MiB；name 8192 项/2 MiB 名字字节；decl 8192；type 8192；field+param 各 32768；当前函数 slot 32768、inst 65536、argument 65536、AST 32768、binding 8192；symbol 32768；fixup 65536；text 8 MiB；initialized data 8 MiB；每函数参数 256 项；每函数 stack frame 256 KiB。总量必须按实际 sizeof/内存布局算出且不超过 v1 的 64 MiB 编译器数据预算；若这些初值组合超额，H 在验收前收紧，不能忽略总账。另保留目标 1 MiB 栈预算。

实际 model.vt 是字段/API 的规范代码，全部表从 ID 1 起，数组容量比逻辑上限多 1。表/计数为 sources/source_count、assemblies/assembly_count、names/name_count、imports/import_count、types/type_count、fields/field_count、decls/decl_count、params/param_count、slots/slot_count、insts/inst_count、args/arg_count、data_objs/data_count、data_relocations/data_relocation_count、symbols/symbol_count、fixups/fixup_count。`add_type(value:Type)->u32` 等 append 接受对应具体记录；`add_arg(value:u32)->u32` 同理。`parameter_slots`/`parameter_slot_count` 由 `add_parameter_slot(u32)->()` 追加。`name([]const u8)->u32` 将字节复制到稳定名字池并 intern；`symbol(name_id:u32,kind:u32)->u32` 按上述规则查找/预声明，name_id=0 每次分配一个匿名符号。

`data_bytes:[8388608]u8` / `data_byte_count:usize` 存初始化对象字节，由 F 按 `data_byte(u8)->()` 追加。Data.first/count 指向此池；`DataRelocation.object` 指 DataId，offset 相对该对象，target 是 SymbolId；M 在最终布局后应用 ABS64。它与 M 产生的 text Fixup 分开，避免 F 预猜段布局。

`fail(code:u32,file:u32,offset:usize)->()` 直接输出错误类别/FileId/字节位置并退出 1，不返回。诊断 write 循环处理短写和 EINTR；零进展、返回超过请求长度或其他错误时放弃剩余诊断，不递归 fail，原失败路径仍退出 1。code 1 E_PARSE、2 E_NAME、3 E_TYPE、4 E_PERMISSION、5 I_UNSUPPORTED、6 I_LIMIT、7 I_INTERNAL、8 E_IO、9 E_LINK。`limit_at(actual:usize,capacity:usize,file:u32,offset:usize)->()` 先输出实际值/上限，再带位置 fail；`limit(actual:usize,capacity:usize)->()` 使用位置 0/0 包装它。有界输入的 actual 是已观察到的所需容量下界，毋须继续读取无限输入。所有阶段都不需以 bool/Result 层层传播不可恢复编译错误。输出产物尚未创建；I/O 发布阶段仍由 L 负责关闭/清理。

`reset_function()->()` 只清函数 arena；`reset_all()->()` 恢复固定基本类型及所有全局表。H 探针由 C0 编译并在 RV64 实跑：基本类型、name 去重、未指定符号升级通过。探针 ELF 的 RW p_memsz=0x1616e48（23,162,440 字节，包括模型记录/字节池），并非仅按源行数猜测。M 使用单一约 17 MiB ELF 缓冲（含 text）而非 text/ELF 双份；加 L 约 5.1 MiB（4 MiB source、1 MiB assembly、manifest 与描述符）、F 的约 3 MiB AST/绑定及其他表后仍须 I 实测核对 64 MiB 总预算，当前不是最终峰值成绩。

## 5 lexer 与前端接口

词法 API 无共享游标，F 自己保存下一字节位置：

```text
lex::next(file:u32, offset:usize) -> model::Token
lex::is(token:model::Token, text:[]const u8) -> bool
lex::bytes(token:model::Token) -> []const u8
lex::decode(token:model::Token, out:[]u8) -> usize
```

next 跳过空白/注释返回当前位置第一项；下一次传 token.end。EOF kind=0，identifier=1，number=2，string=3，punctuation=4；关键词仍为 identifier，符号最长匹配。number 为检查过的 usize 位模式，suffix=0/3/4/5（无/u8/u32/usize），窄值超范围拒绝。bytes 返回源码 token 原文；decode 将字符串解码写入调用方缓冲并返回长度，容量不足 I_LIMIT，不隐式 NUL。decode 输出仅在调用方缓冲期有效；F 要将持久字符串复制进名字/数据池。全部 scan 错误直接 fail，不假装 EOF。Token 带 FileId，start/end 为实际 token 源字节区间。

```text
parse::collect() -> ()
check::function(decl:u32) -> ()
```

collect 根据 model::sources/数量：逐文件登记 module/import/声明头并保存范围；完成所有签名/布局/常量/静态初始化，发布 model types/decls/params/fields/data/symbols。不调用后端。单函数执行 check::function 时只读全局事实，可追加具体 pointer/array/slice/Result type 和字符串 data；填充当前函数 Slot/Inst/Arg 并报告 function_decl、parameter_slots、frame/AST/IR 峰值。出错不发射该函数。

parse/types/check/ir 内部分工由 F 实现，但正式类型判断只做一遍；M 不再根据源码或名字推断语义。AST 是按 tag 区分的具体记录、子节点 ID 和附属列表；表达式位置/值分离。所有实参先生成快照，再发射 CALL；聚合赋值右侧先快照；return/? 在 defer 前保存返回快照。defer 在退出路径展开，只清理离开的作用域；panic 不展开。

## 6 typed IR：线性块标签，机器端无源码知识

每个基本块必须先以 JUMP/BRANCH/RETURN/PANIC 显式终结，才能出现下一个 LABEL；不允许隐式 fallthrough。LabelId 在每函数内独立，范围 1..65536；F 生成的第一条指令为 LABEL 1。终结之后若还有不可达源码，必须开启新 LABEL，最终同样显式终结。

所有数据操作数都为 SlotId；立即数先 CONST 到具体类型槽。每条指令 dst/a/b/c 的含义由 opcode 固定，无需后端猜测。Slot 的 ty 有完整布局，flags bit0 表示参数槽，offset 由 M 分配。`model::parameter_slots` 是 Param 顺序对应的 SlotId 列表。

| op | 字段 | 精确含义 |
| --- | --- | --- |
| 1 CONST | dst,bits | scalar typed constant；聚合不能使用 |
| 2 COPY | dst,a | 完整同类型值复制，包含聚合 |
| 3 LOCAL_ADDR | dst,a,bits | dst 指针，a 为根槽，bits 为已检查字节偏移 |
| 4 SYMBOL_ADDR | dst,a | a SymbolId；dst pointer |
| 5 LOAD | dst,a | a 是 pointer 槽，加载 dst 类型完整值 |
| 6 STORE | a,b | a pointer 槽，存储 b 的完整类型值；允许聚合 |
| 7 OFFSET | dst,a,b | 指针 a 加 usize 字节数 b；权限不变化 |
| 8 BINARY | dst,a,b,aux | aux 见下表，整数/比较操作 |
| 9 UNARY | dst,a,aux | aux=1 ! bool、2 ~ integer、3 checked neg |
| 10 CAST | dst,a | 已检查 integer cast，截断或零扩展 |
| 11 BITCAST | dst,a | 仅批准的 pointer/usize 和 const pointer view |
| 12 BOUNDS | a,b | 两个 usize 槽，要求 a < b，否则 panic |
| 13 CALL | dst,a,b,c | a DeclId，b/c 为 Arg 表起点/个数；unit dst=0 |
| 14 LABEL | a | 当前函数内非零 LabelId |
| 15 JUMP | a | LabelId |
| 16 BRANCH | a,b,c | a bool 槽，b/c true/false LabelId |
| 17 RETURN | a | a 返回快照槽，unit a=0 |
| 18 PANIC | — | Linux 退出 101，防止落空继续执行 |

BINARY aux：1 add、2 sub、3 mul、4 div、5 rem、6 wrapping add、7 wrapping sub、8 wrapping mul、9 shl、10 shr、11 bitand、12 bitor、13 bitxor、14 eq、15 ne、16 lt、17 le、18 gt、19 ge。&&/|| 必须由 F 降为短路控制流，不存在 eager logical IR。比较输入类型一致，结果 bool。add/sub/mul 的 u8/u32/usize 都 checked；wrapping 按目标位宽截断。shift 的 b 与种子一致且范围小于位宽；unsigned neg 非零触发 panic。slice 是普通 16 字节聚合，字段读取降为 LOCAL_ADDR/LOAD；Result tag 位于 offset 0，payload 按 Type.first 对齐，匹配降为 LOAD/BRANCH。

M 验证表 ID、完整类型、标量宽度、同型复制、指针目标、参数/结果签名、label 唯一定义/引用和合法 terminator；拒绝 malformed IR 返回 I_INTERNAL，不发生静默越界。前端创建的内部 OFFSET 可以用于布局投影，但不能变成 source pointer 任意 cast。

## 7 后端、链接和驱动接口

```text
image::reset() -> ()
image::symbol(name:[]const u8) -> u32
image::fresh() -> u32
image::define(symbol:u32, region:u32, offset:usize) -> ()
image::text_position() -> usize
image::word(bits:u32) -> ()
image::data(symbol:u32, mutable:bool, alignment:usize, bytes:[]const u8, zero_fill:usize) -> ()
image::abs64(symbol:u32, object_offset:usize, target:u32, addend:usize) -> ()
image::finish(entry:u32) -> []const u8
rv64::start(entry:u32) -> u32
rv64::function(decl:u32) -> ()
s0::parse(file:u32, bytes:[]const u8) -> ()
```

image::reset 只清 M 输出状态和 text fixups，不清 F 的 symbols/data_objs/data_bytes/data_relocations。image::symbol 包装 model::name 和 model::symbol(kind=0)；fresh 为匿名 text-label symbol。image::data/abs64 是 M 手工 IR/数据夹具所需接口，可追加到 model 对象表；F 不调用 M。finish 自动读取 model Data 表，完成布局、未定义符号/回填检查及 ELF 写入最终缓冲，返回稳定视图。失败立即 fail，没有假成功空 ELF。

rv64::start 接受语言入口函数的 SymbolId，发射保存初始 sp 于 x27 的 `_start` 和 panic 支持，返回机器启动 SymbolId；入口语言签名必须 safe、无参数、u32 普通函数，由驱动/F 验证。function 消费当前 typed IR，不保留函数 arena 地址。s0::parse 接受稳定 FileId，assembly 编号为 source_count+1 开始；文件字节明确传递，不依赖 L。finish 的 entry 是 rv64::start 返回的机器 SymbolId；必须位于 RX。

内部 ABI 与 B2 相同：scalar 参数一个整数位置，聚合传快照地址，unit/零大小参数不占位置；聚合结果第一个隐藏结果地址，后续参数顺移；前八个位置 a0..a7，其余 caller 栈上传递，sp 16 对齐。callee 将参数复制到自己的参数槽。scalar 返回 a0；聚合返回写入隐藏结果。x27 保持 Linux 初始栈指针，其余所有 callee-saved 均正确保留；只用 caller-saved temps 可以简化保存。固定远转移临时寄存器 x31，与 B2 实现一致（v1 早期 x5 提案不再直接套用）。

机器编码、固定 8/12 字节跳转、ABS64/PCREL、0x10000 基址、RX/R/RW 三段、BSS 不写文件、64 MiB 映像跨度都延续 B2。不同的合法栈布局/指令选择可以使 C0 产物 C1 与 C2 不同；必须比较 C2/C3，不要求 C1/C2 字节相等。行为对照同一 VT 程序必须一致。初始化对象字节、补码数和 ELF 字段逐字节序列化，不能用目标地址作为稳定编号。

驱动顺序：读 manifest → 输入快照 → reset/注册 sources 和 assemblies → collect → image::reset/rv64::start → 按 DeclId 遍历普通函数（check + emit，逐函数 reset）→ 解析 s0 → 验证 entry/finish → 完整写出/关闭。check 模式执行前端全部函数及必要 extern/入口结构检查，不写 ELF。

## 8 验收和规模账

每个节点交候选 SHA-256、命令/退出码、输入版本和独立审查记录。I 除固定点外须覆盖：

1. 实际 C1/C2/C3 编译新源码（常量、分支、循环、跨函数、聚合快照、defer、Result、静态/切片/模块/extern）；C0 同输入同退出行为。
2. 拒绝语法/未知名字/错类型/权限/未定义汇编符号，I_UNSUPPORTED 与 I_LIMIT 区分，有 FileId/字节范围且无新 ELF。
3. 非模板扰动：用户源码算式/字符串/控制流改变；编译器诊断文本源码改变后由 C1 产出新 C2 并观察改变。
4. 短读写/EINTR/输入丢失/容量错误/输出已存在/输出与输入冲突，不能损坏输入或保留失败半产物。
5. C2/C3 比较包含完整文件，生产执行隔离宿主编译工具；从只含声明输入的干净树重建。
6. 单函数 frame/AST/IR、全部表峰值、source/text/data/bss/fixup 数量；Rust seed、VT 编译器、运行库、s0、生成表、测试分开计数。实际自编译超预算时必须报告并修正架构/实现，不隐藏增长。

本合同的验收状态独立保存在 `docs/workflows/bootstrap-b3/`，本文不提前声称自编译成功。
