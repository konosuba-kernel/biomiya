# Core S S-next 0.1 使用参考

文档修订 5，2026-10-15。ISA仍为RV64小端；编译器运行于Linux，可输出linux-user、suba-user、suba-boot、suba-kernel四种固定profile。本修订的C64候选SHA-256为 `22d2c81fd0dbfc6de6b3b13fb2f485b1c64913849d9e59eeabe8c2757d27ebf4`，源文件上限64；独立验收状态见[C64交付记录](../history/source-cap64/report.md)。原K0编译器 `049c6e094ccd044fba3b5d357583013b77f0e8e9a98d431ad919e02b0cda2144` 及JSON登记的更早身份仍为32源文件，见[K0记录](../../../suba/docs/workflows/k0/DELIVERY.md)。修订5只增加源文件容量，不改变核心语言语义或其他容量。[修订3维护证据](../history/s-next/compiler/maintenance-2026-10-14.json)和[原发布证据](../history/s-next/compiler/release.json)保留。

本页描述 `compiler/` 的输入语言。B3.1 是冻结的引导版本，两者不兼容；旧 Result、问号和块式 defer 不属于本版。Q 是今后的维护实现，`bootstrap/transition/`的 P 仅供一次性引导，Rust 种子不追踪本版语义。

## 1 使用方式

保存为 `main.vt`：

<!-- example: hello/main.vt -->
```vt
module app;
fn run() -> u32 { return 42; }
```

保存为 `program.build`：

<!-- example: hello/program.build -->
```text
format 1
profile linux-user
entry app::run
source main.vt
output program.elf
```

在这些文件所在目录执行：

```sh
qemu-riscv64 -s 1048576 compiler.elf check program.build
qemu-riscv64 -s 1048576 compiler.elf build program.build
qemu-riscv64 program.elf
```

程序退出 42；Linux shell 的退出状态仅保留低 8 位。真实 RV64 Linux 可直接执行 ELF。编译器 CLI 是 `build|check MANIFEST [--blocking POLICY]`，没有 `--version`；版本依据完整 ELF SHA-256。统计前缀为 `S-next-0.1`。

清单显式列出全部 `.vt` 与可选 `.s0` 输入；没有包搜索、隐式标准库、libc 或外部链接器。入口为普通安全函数，写完整模块名；用户profile无参数、返回u32，机器profile见下表。`format 1`、`profile`、`entry`、`output` 各一次，source 1–64个（旧编译器身份为1–32个），assembly 0–8个；路径不得重复。

路径相对**进程工作目录**。最长 96 字节；禁止绝对路径、空分量、`.`、`..`、反斜线、空白与控制字符。每条有效清单行恰好两个词；允许空行和整行 `#` 注释。输出父目录必须已经存在。

`check` 完成语义、目标检查与链接，不写 ELF；`build` 独占新建输出，拒绝覆盖已有文件。类型、链接或要求的 Blocking 分析失败时不发布新 ELF。保存失败会关闭文件并尝试删除本次新建输出；删除失败时报告可能残留的路径。完整写入后才设置可执行权限；关闭失败仍属于保存失败。不承诺并发替换输出路径安全或突然断电事务。

### 固定目标与启动桥

编译器的运行环境由其构建清单选择`runtime::host`实现；输出profile由被编译程序的清单选择。四种profile都直接生成三个RX/R/RW的ELF段，无外部链接器，也没有任意链接脚本。

| profile | text基址 / 映像上限 | entry签名 |
|---|---|---|
| linux-user | 0x10000 / 64 MiB | `()->u32` |
| suba-user | 0x10000 / 64 MiB | `()->u32` |
| suba-boot | 0x80200000 / 2 MiB | `(hart:usize,dtb:usize)->u32` |
| suba-kernel | 0x80400000 / 64 MiB | `(info:usize)->u32` |

boot/kernel还必须提供`bind panic=module::function`，kernel必须提供`bind trap_dispatch=module::function`。每行仍恰好两个词。panic是安全普通`(u32)->u32`，trap_dispatch是安全普通`(usize)->usize`。重复、未知、缺失、错签名绑定拒绝；用户profile拒绝bind，boot拒绝trap_dispatch。没有通用export或可取函数地址。

机器profile的最低PT_LOAD首字节等于e_entry。入口先关闭SIE、清BSS、建立16字节对齐的128KiB栈（计入窗口）、将x27清零，再保留a0/a1进入普通函数。函数返回进入panic(reason=102)，语言checked panic为101；panic返回后跳S0提供的`__suba_halt`，该符号必须存在且不返回。独立紧急栈、guard、完整trap保存由系统实现，编译器没有代为提供。S0可call`__suba_trap_dispatch`，a0传现场地址并返回待恢复现场地址；桥只是保留ra的尾跳，不保存中断现场。

linux-user的x27保留Linux初始sp；suba-user的x27保存loader在a0传入的只读参数块地址，sp由loader提供。suba-user正常返回及panic101进入运行库的`__suba_exit`（a0退出码），缺少它则链接失败。它必须实现suba真实退出协议，不能把Linux退出包装伪装为suba服务。当前K0只完成这类产物的布局/链接边界；suba用户装载器和系统内编译仍待后续阶段。

## 2 文件、名字与权限

每个文件必须以 `module a::b;` 声明模块，后接 import 和顶层声明。一个模块可以分布于多个显式输入文件。导入图不得有环；同模块函数可相互递归，具名类型先登记身份。

```text
import utilities::bytes;
import utilities::bytes as b;
```

默认别名为路径末段。通过 `b::item` 访问导入模块；没有选择性/通配导入，也没有特殊 `root::` 根路径。名字先查局部绑定，再查当前模块或显式导入。局部绑定遮蔽函数后不可调用该名字。

顶层声明默认模块私有；`pub` 开放声明。公开函数、字段和枚举载荷的类型必须可公开。`pub struct` 可以含私有字段：外部不能读取、写入或构造这些字段，也不能借 `zero` 绕过此限制。枚举的标签随类型公开，不能直接投影载荷。

标识符限 ASCII 字母或 `_` 开头，后接字母、数字或 `_`，区分大小写。关键字不可作普通名字；`root`、`core` 保留为模块首段、顶层声明名和导入别名，不能用于这些位置。源码须为合法 UTF-8，无 BOM；UTF-8 注释、字符串可用。支持 `//`、可嵌套 `/* */` 注释及 LF/CRLF/CR 换行。

整数支持十进制、`0x`、`0b`；后缀 `_u8`、`_u32`、`_usize`。数字间可用单个 `_`，不允许连续或首尾分隔符。负号为运算符。

字符串为静态存储的 `[]const u8`，长度按字节计，无隐式尾零。转义：`\0 \n \r \t \\ \" \xHH`。解码字节不保证仍为 UTF-8；没有字符字面量、插值或 Unicode 转义。空字符串具有非零哨兵地址；`zero` 切片的地址为零、长度为零。

## 3 类型与布局

| 类型 | 大小 / 对齐（字节） | 说明 |
| --- | --- | --- |
| `()` | 0 / 1 | 唯一值 `()` |
| `bool` | 1 / 1 | 与整数不同 |
| `u8`、`u32`、`usize` | 1/1、4/4、8/8 | 无符号整数 |
| `*T`、`*const T` | 8 / 8 | 可写/只读原始指针，可为空 |
| `[N]T` | N×sizeof(T) / alignof(T) | N 必须为布局前 usize 常量且不超过 2³²−1 |
| `[]T`、`[]const T` | 16 / 8 | 数据指针与 usize 长度 |
| `struct` | 自然字段布局 | 声明顺序，字段及尾部对齐 |
| `enum` | u32 标签 + 共用载荷区 | 见下节 |

结构与枚举采用名义类型；字段相同不构成同一类型。空结构和零长度数组可用。按值递归拒绝，包括零长度数组中的递归；经指针或切片的递归允许。`sizeof(T)`、`alignof(T)` 返回 usize，仅用于运行期表达式中的布局查询，不能用于 const 或数组长度。

值按值复制。结构、数组、枚举复制自身，指针/切片只复制地址或描述符；没有自动析构、move、借用证明、垃圾收集或隐式装箱。没有 packed、用户对齐属性或 offsetof。

## 4 构造与局部定型

```text
let fixed:T = expression;
var changing:T = expression;
let inferred = expression;
make Point { x: 1, y: 2 }
```

所有局部绑定必须初始化。let、参数不可重新赋值；var 可赋值。绑定不可变不意味着它指向的存储不可写。同作用域重复名字拒绝，内层可遮蔽。函数参数、返回、字段、const 和 static 均须写类型；局部类型可省略。

定型只沿一个方向传播，没有默认整数类型或回溯：有期望类型时检查操作数；否则先确定左操作数，再检查右操作数。比较的 bool 结果不提供整数上下文。因此 `let n = 3_u32; n == 0` 可用；`let n = 3;`、无上下文的 `0 == n` 拒绝。`as` 不向左侧反向传类型。

结构构造须写 `make`，字段必须恰好出现一次且均可访问；按书写顺序求值。数组列表 `[a,b]`、重复 `[value; N]` 需要完整数组类型上下文。重复项仅求值一次，即使 N 为零；N 同样须为布局前 usize 常量。

`zero` 需要期望类型。unit、bool、整数、指针、空切片，以及递归满足条件且字段可访问的数组/结构允许。枚举仅当首标签无载荷时允许；首标签携带 unit 仍不允许。zero 不执行构造器或模块初始化代码。

<!-- example: zero/main.vt -->
```vt
module app;
struct Point { x:u32, y:u32 }
fn run() -> u32 {
    var p:Point = zero;
    p.x = 20;
    let q = make Point { x:p.x, y:22 };
    return q.x + q.y;
}
```

结果为 42。数组和聚合不支持整体相等比较。

## 5 具体枚举与匹配

每个标签无载荷或携带一个具体类型。多字段载荷用具名结构；不支持用户泛型、嵌套模式、守卫或模式类型注解。

<!-- example: enum/main.vt -->
```vt
module app;
struct Pair { first:u32, second:u32 }
enum Answer { Empty, Value(Pair), Failed(u32) }
fn run() -> u32 {
    let answer = Answer::Value(make Pair { first:20, second:22 });
    match answer {
        Answer::Empty => { return 0; },
        Answer::Value(pair) => { return pair.first + pair.second; },
        Answer::Failed(code) => { return code; },
    }
}
```

结果为 42。无载荷标签写 `Answer::Empty`，不能写 `Empty()`；带载荷标签必须且只能传一个值。显式 unit 载荷以 `Tag(())` 构造、`Tag(value)` 或 `Tag(_)` 匹配。

匹配先对输入求值并保存快照。枚举每个标签必须明确覆盖一次，禁止整体 `_` 分支；载荷绑定为该分支的不可变局部变量。bool 可用 true/false 穷尽，或使用最后的 `_`；整数匹配须有最后的 `_`。不提供 match 表达式或分支公共类型推导。

整数模式仅接受整数字面量，不接受命名 const、算式或转换。重复值拒绝；默认分支必须最后出现。

标签按声明顺序从 0 开始。载荷起点为 4 向所有载荷最大对齐（至少 4）取整；载荷区取最大载荷大小，整体再按最大对齐补齐。无 niche 优化或聚合 C ABI。空枚举、重复标签、按值循环拒绝。unsafe 伪造的标签/指针不由普通类型规则保证有效。

## 6 函数、运算与执行顺序

```text
fn f(arg:T) -> R { ... }
unsafe fn f(arg:T) -> R { ... }
```

调用为直接函数或 extern 调用；没有函数值、间接调用、闭包、方法或重载。所有正常结束路径显式 `return expression;`，包括 `return ();`。没有尾表达式或 `return;`。无限 loop 可以不返回；编译器不证明递归或任意调用永不返回，`core::panic();` 本身不替代语法上的返回路径检查。

普通操作数、调用实参从左到右求值；每个实参立即保存值，后续参数副作用不能改变已保存的聚合或标量。赋值先求左侧地址并完成检查，再求右值，最后存储。

| 操作 | 规则 |
| --- | --- |
| `+ - *` | 同类型整数；溢出/下溢 panic |
| `+% -% *%` | 按类型位宽回绕 |
| `/ %` | 无符号；除零 panic |
| `<< >>` | 同整数类型，移位量须小于位宽；左移丢弃移出位 |
| `& \| ^ ~` | 整数位运算 |
| `! && \|\|` | bool，运行期后两者短路 |
| `== !=` | 同类型标量，包括 unit、bool、指针；不比较聚合 |
| `< <= > >=` | 同类型整数 |
| `as` | 整数拓宽零扩展，缩窄截位 |

仅有无符号整数；一元负号仍执行 checked 减法，因此对非零整数触发下溢。使用回绕减法明确表达模运算。

优先级从低到高为下表；同级二元运算从左结合。位运算比比较结合更紧，不能按 C 的优先级推断。

| 优先级（低→高） | 运算 |
| --- | --- |
| 1、2 | `\|\|`；`&&` |
| 3、4 | `== !=`；`< <= > >=` |
| 5、6、7 | `\|`；`^`；`&` |
| 8 | `<< >>` |
| 9 | `+ - +% -%` |
| 10 | `* / % *%` |
| 11 | `as` |
| 12 | 一元 `! ~ - *` |
| 13 | 调用、字段、下标 |

语句控制流包括 if/else、while、loop、match、break、continue。if/while 条件须 bool；无 for、标签跳转或异常。

## 7 defer 与失败处理

只接受 `defer cleanup(args);`。被调用者须为声明的直接函数/extern，返回 unit；内建、枚举构造、代码块和非 unit 清理拒绝。

登记时检查调用权限，从左到右求值并保存实参，全部成功后才登记。指针只保存地址，不延长目标寿命。作用域正常结束、return、break、continue 按离开作用域逆序执行；循环每次迭代分别登记/清理。return 先保存返回值。panic 直接终止，不展开；清理自身 panic 后不再执行后续清理。

没有 Result 特例或 `?`。用具体结果枚举和显式 match 传播错误。若 close 失败必须影响结果，显式调用并检查；unit 清理函数自行明确报告、忽略或终止策略。defer 不证明资源所有权或回滚正确。

## 8 指针、切片与 unsafe

解引用、切片元素访问、指针算术、原始地址构造、可变静态读写及 unsafe/extern 调用要求显式 unsafe 块。unsafe fn 只约束调用者，函数体仍须显式 unsafe 块。

`*T` 可写，`*const T` 只读；兼容拼法 `*kernel T` 不提供独立地址空间语义。`[]T`、`[]const T` 的 `.data`、`.len` 本身只读。数组不隐式转切片；指针/切片不隐式转只读。

| 内建 | 作用 / 权限 |
| --- | --- |
| `core::addr_of(place)` / `addr_of_mut(place)` | 普通根取址安全；位置求值自身的危险访问仍须 unsafe；后者要求可写 |
| `core::addr(pointer)` | 地址位转 usize |
| `core::ptr::<T>(address)` | usize 构造指针；unsafe |
| `core::ptr_const(pointer)` | 显式转只读指针 |
| `core::ptr_add(pointer,n)` / `ptr_add_bytes(pointer,n)` | 前进 n 个元素/字节，n 为 usize；unsafe |
| `core::slice_of(array)` / `slice_of_mut(array)` | 数组位置构造切片，后者要求可写 |
| `core::slice_from_raw_parts(pointer,len)` / `_mut` | 原始指针与 usize 构造切片；unsafe |
| `core::slice_const(slice)` | 转只读切片 |
| `core::panic()` | 立即退出，目标退出码 101 |

数组/切片下标有界检查；原始指针有效期、对齐、存储权限、设备属性由程序保证。没有通用 read/write/copy_bytes/fill_bytes/atomic 内建。设备与原子使用有明确宽度、排序和 ABI 的具体汇编适配，不把普通内存访问当作 MMIO。

## 9 常量与静态初始化

`const N:usize = expression;` 只允许整数或 bool：字面量、之前可见常量、有限标量运算及整数转换。禁止调用、地址、布局查询、块和 match。常量求值检查并计算两侧，不以运行期短路规则跳过除零等错误。当前模块的后续 const 不可引用；导入模块的常量先完成。同模块多文件以清单顺序确定 const 的先后。

`static NAME:T = initializer;` / `static mut NAME:T = initializer;` 支持标量、字符串、make、数组、具体标签、zero，以及指向静态根对象的地址重定位；不执行用户函数。子项全部检查，零重复不豁免错误。无自动模块构造器，由程序入口显式初始化复杂状态。

## 10 外部边界与 Blocking

`extern abi(C) fn ...;` 支持当前 RV64 标量/指针调用约定：参数限 bool、u8、u32、usize 或指针，unit 只可作结果；不提供结构/枚举外部 ABI、变参或完整 C 类型系统。`.s0` 是有限 RV64 汇编适配输入，指令和重定位边界见 [B3.1 的外部调用说明](../../bootstrap/frozen/core-s/docs/reference/b3.1.md#12-外部函数与-s0)；本版没有增加硬件整数宽度、函数指针或隐式运行库服务。

目标约定保留 x27，外部函数必须保持它。外部符号按声明短名链接，同短名的多处 extern 指向同一符号；全部 extern 均须有定义，即使没有调用也不豁免。外部函数的寄存器保存和栈约定由适配代码承担。

`--blocking POLICY` 依据直接调用关系计算 may-block。Verified、Violation、Unknown 分别表示要求已证明、发现阻塞路径、信息不足；未知 extern 不默认为安全。分析只读已验证 IR，不改变生成 ELF。要求无法通过时不发布输出。

最小策略文件 `policy` 可检查第 1 节的程序：

```text
format 1
require app::run
```

运行 `bmc check program.build --blocking policy`。`format 1` 必须先出现且只出现一次；至少一个 `require 模块::函数` 要求该普通函数可证明不阻塞。`nonblocking 模块::外部函数` 与 `may_block 模块::外部函数` 只为 extern 提供外部合同，前者是调用者承担的假设。相同链接符号的多个 extern 共用一条合同，重复或冲突合同拒绝；未知名字、重复 require 和缺少 require 也拒绝。空行与整行 `#` 注释允许，策略最多 16 KiB。Verified 不证明终止、IRQ 安全或外部合同真实性。

## 11 限制与版本身份

本版按具体ELF身份区分容量：[身份清单](s-next-0.1.json)中的 `source_capacity_by_compiler` 精确对应各SHA-256。共同语言版本号不能替代这一容量身份；旧32文件编译器不会因参考更新而获得64文件支持。

输入硬容量为清单 16 KiB、源码总量 4 MiB、汇编总量 1 MiB。C64候选主要容量：64 源文件（旧登记身份为32）、8 汇编文件、8192 名字/声明/具体类型、32768 字段/参数、每个结构最多 256 字段、每个枚举最多 256 标签；逐函数 32768 AST 节点、32768 语义槽、65536 IR 指令/标签/保存实参、256 形参、8192 局部绑定、1024 已登记清理、256 层循环。头部语法与当前函数共享 AST/类型语法容量，不是各有一份满额预算。

AST 表达式深度、类型准备、布局/常量计算各有明确递归限制；容量超限报告 I_LIMIT。编译器自编译以 1 MiB guest 栈、64 MiB 映像/可写段为验收预算，不是承诺任意用户程序的递归或峰值 RSS。

直接 ELF 输出也有用户程序限制：映像跨度不超过 64 MiB，文本不超过 8 MiB，初始化数据缓冲不超过 8 MiB。静态 BSS 不占同等文件字节，但仍受映像跨度约束；这些不是仅对编译器自身统计的目标。

诊断形如 `E_TYPE file=1 byte=42`：文件序号与字节偏移不是行列号。词法/语法、名字、类型、权限、I/O、链接错误分别使用对应 E_*；I_LIMIT 表示容量，I_UNSUPPORTED 表示明确未支持，I_INTERNAL 表示内部合同损坏。编译失败通常退出 1；用户程序 panic 退出 101。

保存失败在 `E_IO file=0 byte=0` 前补充 `save path=… phase=… errno=… close_errno=… cleanup_errno=…`。phase 为 create/write/chmod/close，errno 保留主错误；后两项为关闭与清理错误，零表示没有相应错误。清理失败还打印 `residual=…`；不能仅凭输出文件存在判断构建成功。记录构造的未知、重复或不可见字段定位到字段名，遗漏字段定位到整个构造，字段值自身的错误仍定位到该值。

不支持：有符号/浮点/额外整数宽度、用户泛型、宏、函数指针、闭包、对象、trait、隐式转换、借用检查、自动内存/资源管理、通用编译期执行、多目标或优化框架。Linux-user 自编译不等于 suba 内自举；真实硬件切片的监控程序与平台成本另行列账。

本页完整示例由 `tools/check_next_reference.py` 验证；语言规则正反例见 `tests/language/p3/cases.json`。当前候选身份、固定点、目标测试与成本以K0记录和执行记录为准；旧release.json是历史基线。

## 12 S0机器边界扩展（K0）

S0仍为有限汇编适配器：数字寄存器x0..x31、标签、call/jump/address；没有GNU宏、section、任意数据指令或符号CSR名字。所有寄存器/数值/多余操作数均检查。

| 新语法 | 合同 |
|---|---|
| `sh xS, offset(xB)` | offset为−2048..2047，单次16位存储 |
| `csrrw/csrrs/csrrc xD, csr, xS` | csr为无符号0..4095 |
| `csrrwi/csrrsi/csrrci xD, csr, zimm` | zimm为0..31，与csr独立验证 |
| `sfence.vma xAddress, xAsid` | 两个寄存器均需明确给出 |
| `sret`、`wfi`、`fence.i` | 无操作数；权限/平台责任由机器层承担 |
| `fence` | 无操作数，固定完整iorw,iorw屏障 |
| `align N` | N为4..4096的2幂，填充32位NOP，影响随后标签地址 |

新增unsigned字段接受十进制或小写0x十六进制，不接受负号或溢出后截位。支持机器指令不表示用户程序获得S-mode权限。实际CSR、MMIO及完整现场保存需要系统合同和QEMU验证。
