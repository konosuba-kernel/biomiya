# KTL 语言参考手册

> **版本**：0.4.1-rc3  
> **状态**：Lean Core Candidate  
> **名称**：KTL 为暂名  
> **定位**：面向裸机、操作系统内核与底层系统软件的教学型系统编程语言  
> **首个规范目标**：RV64  
> **版本主题**：Feature Purge

---

# 第一部分　设计目标

## 1. KTL 是什么

KTL 是一门静态类型、freestanding-first 的系统编程语言。

它面向：

- 操作系统内核；
- boot 与 early runtime；
- 驱动程序；
- 裸机程序；
- hypervisor；
- runtime；
- 其他直接接触内存、ABI 与硬件状态的底层程序。

KTL 不假设存在：

- libc；
- garbage collector；
- 默认 heap allocator；
- exception runtime；
- dynamic loader；
- runtime reflection。

最小程序可以直接运行于裸机：

```text id="2m1i72"
@export("_start")
fn start() -> ! {
    loop {
        arch::riscv::wfi();
    }
}
```

---

## 2. 设计原则

### 2.1 机器保持可见

KTL 允许直接表达：

```text id="5bygzz"
integer
address
memory layout
raw pointer
ABI
MMIO
atomic
linker section
CPU state
architecture operation
```

高级抽象不得偷偷要求：

- heap；
- GC；
- exception unwind；
- dynamic object runtime。

---

### 2.2 少概念优先

KTL 不追求接受所有理论上安全的程序。

如果某项能力需要引入：

- general lifetime solver；
- alias theorem proving；
- complex trait solver；
- whole-program typestate；
- dynamic metaprogramming；

Core 可以直接不支持。

---

### 2.3 安全边界显式

需要调用者承担额外机器级前置条件的操作必须位于：

```text id="52h97v"
unsafe {
    ...
}
```

中。

`unsafe` 不关闭：

- 类型检查；
- Copy / Move；
- resource；
- borrow；
- CFG ownership；
- address-space；
- effect；
- Sized 检查。

---

### 2.4 普通失败不是异常

可选值：

```text id="ft95vv"
Option<T>
```

可恢复错误：

```text id="du603r"
Result<T, E>
```

不可恢复错误：

```text id="f0ght2"
panic(...)
```

KTL 不定义：

```text id="bt8cru"
throw
catch
exception unwind
panic recovery
```

---

### 2.5 普通操作具有规定语义

例如：

```text id="7zc8av"
integer overflow       -> panic
division by zero       -> panic
invalid shift          -> panic
array out of bounds    -> panic
```

只有实际执行某项 unsafe operation 并违反其规定前置条件时，程序才离开 KTL specified semantics。

---

# 第二部分　Feature Purge

## 3. rc3 从 Core 删除的机制

以下能力**不属于 KTL 0.4 Core**：

```text id="rmga2t"
Pin

user-defined trait

const fn

general bitcast

may_alloc effect
may_fault effect
may_sleep effect

TrapResident as a Core type
@stack_limit as a Core annotation
@no_recursion as a Core annotation

borrow-carrying instantiation
of user-defined generic parameters
```

---

## 4. 删除 Pin

Core 不再定义：

```text id="mp7bp1"
Pin<&T>
Pin<&mut T>
pin
pin_mut
```

长期地址稳定由 library owner 保证，例如：

```text id="8wre6l"
TaskSlot
PinnedPage
DmaRegion
```

这些 owner 内部可以通过 raw pointer 与 unsafe invariant 实现。

intrusive data structure 是否真的需要语言级 Pin，由未来真实实现决定。

---

## 5. 删除用户 Trait

Core 不再定义：

```text id="yg4wj2"
trait
impl Trait for T
dynamic trait object
```

KTL 保留：

```text id="zq119z"
generic function
generic struct / enum
inherent impl
T: Copy
```

其中：

```text id="yy4k40"
Copy
```

是 compiler-known capability，不是用户 trait。

接口可以使用：

- module；
- generic concrete type；
- function pointer；
- ops struct；

表达。

例如：

```text id="vkk7qj"
struct BlockDeviceOps {
    read:
        fn(
            *Device,
            u64,
            []u8,
        ) -> Result<(), IoError>,
}
```

---

## 6. 删除 `const fn`

Core 保留有限 const expression evaluator，但不允许用户定义编译期函数。

不存在：

```text id="xqwo99"
const fn
```

这样第一版 compiler 不需要：

- const-call graph；
- compile-time function interpreter；
- const recursion analysis。

---

## 7. 删除通用 `bitcast`

KTL 0.4 Core 不定义：

```text id="654el9"
bitcast<T>(...)
```

整数间机器转换使用：

```text id="632ylg"
cast<T>
checked_cast<T>
```

对象 representation 重解释通过：

```text id="h0q0f4"
raw storage
addr
ptr<T>
```

等 machine-level mechanism 完成。

这避免：

- padding；
- bit-validity；
- aggregate bitwise domain；
- ownership-class transmutation；

进入 Core 类型系统。

---

## 8. Effect 缩减为单一 `may_block`

Core 只跟踪：

```text id="bo9h9k"
may_block
```

effect。

以下不再是 Core function effect：

```text id="6u8fm2"
may_alloc
may_fault
may_sleep
```

allocation safety、fault reliability、stack usage、trap residency 等属于 verifier / platform concern。

---

## 9. Verifier 属性移出 Core

以下不属于 Core language semantics：

```text id="99ekz3"
TrapResident
@stack_limit
@no_recursion
```

它们可以由未来：

> Kernel Verification Supplement

定义。

Core compiler 不需要理解其类型语义。

---

## 10. Generic Borrow Purge

用户定义 generic parameter **不能被 borrow-carrying type 实例化**。

因此：

```text id="u6m3oz"
struct Box<T> {
    value: T,
}
```

可以：

```text id="6mda9s"
Box<u32>
Box<Task>
Box<Frame>
```

但不能：

```text id="olodw4"
Box<&Task>
Box<[]const u8>
Box<Option<&Task>>
```

这一限制同时适用于用户定义 generic function 的 type argument。

例如：

```text id="vtq324"
fn identity<T>(
    value: T,
) -> T
```

不能使用：

```text id="gzwcm2"
identity<&Task>(...)
```

但可以正常写：

```text id="3c3on3"
fn identity_ref<T>(
    value: &T,
) -> &T {
    return value;
}
```

此时 generic parameter `T` 是 pointee type，而不是 borrow-carrying type。

---

## 11. Borrow-transparent Core 类型

以下 Core / required-core composition 仍然可以携带 borrow：

```text id="m6hgi2"
Option<&T>
Result<&T, E>

tuple containing borrow
array containing borrow

concrete user struct
with explicitly borrowed fields
```

因此删掉 generic-borrow machinery 并不意味着 KTL 无法表达 borrowed aggregate。

---

# 第三部分　规范层级

## 12. Language Intrinsic

编译器必须理解：

```text id="a7yaea"
MaybeUninit
UnsafeCell
Atomic

raw pointer operations

volatile operations
MMIO operations

forget

sizeof
alignof
offsetof

cast
checked_cast

panic
```

---

## 13. Required `core` Library

必须提供：

```text id="o395nm"
Option
Result

replace
swap

integer MIN / MAX

basic slice helper
```

`replace` 与 `swap` 只定义于：

> 非 borrow-carrying T。

这样它们不会成为 borrow escape 的隐藏通道。

---

## 14. Platform Library

不是语言机制：

```text id="sxvraz"
OwnedSlot
TaskSlot
PinnedPage
DmaRegion

SpinLock
Mutex

copy_from_user
copy_to_user

device abstraction
scheduler abstraction
filesystem abstraction
```

---

## 15. Architecture Supplement

RV64 Architecture Supplement 必须定义：

```text id="r2hfpc"
wfi
cpu_relax

CSR access

trap entry ABI
TrapFrame
trap return

enter_trap_handler

device read/write fence

inline asm grammar
register operands
clobbers
```

---

# 第四部分　源文件与模块

## 16. 源文件

UTF-8。

建议扩展名：

```text id="7quklh"
.kt
```

---

## 17. 注释

```text id="ynmynm"
// line comment

/*
    block comment
*/

/// documentation comment
```

---

## 18. Module

```text id="75wpqq"
module mm::page;
```

路径：

```text id="zpsio5"
::
```

---

## 19. Visibility

声明默认 module-private。

使用：

```text id="0dz6qr"
pub
```

导出。

---

## 20. Import

```text id="ihye9u"
import mm::page;

import mm::page::{
    Page,
    alloc,
};
```

---

## 21. Type Alias

```text id="ne1t2r"
type IrqHandler =
    fn(&mut TrapFrame) -> ();
```

alias 是透明的。

需要 nominal type：

```text id="3s1bdd"
struct PhysAddr(usize);
```

---

## 22. Enum Variant

可以完整写：

```text id="aeceun"
TaskState::Ready
```

上下文唯一时可简写：

```text id="qj7kqc"
Ready
```

存在歧义时必须限定。

---

# 第五部分　基础类型

## 23. 整数

```text id="zx5w9b"
u8
u16
u32
u64

i8
i16
i32
i64

usize
isize
```

RV64：

```text id="yiwdzu"
sizeof(usize) == 8
sizeof(isize) == 8
```

---

## 24. 当前不支持

```text id="1zyvhl"
u128
i128

floating-point types
```

---

## 25. Literal

```text id="hohcsq"
123
0xff
0b1010
```

支持 `_` 数字分隔符。

---

## 26. Integer Suffix

```text id="bhyar7"
1_u8
1_u16
1_u32
1_u64
1_usize

1_i8
1_i16
1_i32
1_i64
1_isize
```

无上下文时默认：

```text id="pfprqd"
i32
```

---

## 27. Signed MIN Literal

紧邻 unary `-` 时允许 magnitude 达到：

```text id="fcy3ea"
MAX + 1
```

以形成 `MIN`。

```text id="42hcic"
-128_i8      // OK
128_i8       // error
```

---

## 28. Integer Constants

```text id="ijvh87"
T::MIN
T::MAX
```

例如：

```text id="ueownw"
i32::MIN
usize::MAX
```

---

## 29. Bool

```text id="hkq9fs"
bool
true
false
```

整数不会隐式变成 bool。

---

## 30. Unit

```text id="oo6mxd"
()
```

---

## 31. Never

```text id="9w5omz"
!
```

表示控制流不会正常返回。

---

## 32. String Literal

```text id="9ihy0x"
"hello"
```

类型：

```text id="hhux45"
[]const u8
```

来自 static storage。

---

## 33. `null`

`null` 只能变成 raw pointer。

不能变成：

```text id="038l3m"
&T
&mut T
function pointer
```

---

# 第六部分　Sized

## 34. 所有 Object Type 必须有限 Sized

除 `!` 外，所有可以形成 KTL object 的 type 都必须具有有限且编译期确定的：

```text id="tu64q0"
sizeof(T)
alignof(T)
```

---

## 35. 直接递归非法

```text id="70lrt4"
struct Node {
    next: Node,
}
```

非法。

```text id="bjvrj9"
enum List {
    Nil,
    Cons(i32, List),
}
```

非法。

---

## 36. Indirection 递归合法

```text id="ks3wo1"
struct Node {
    next: *Node,
}
```

合法。

---

# 第七部分　表达式与求值

## 37. 求值顺序

从左到右。

```text id="sfb11w"
foo(a(), b(), c());
```

执行：

```text id="u4c1cm"
a()
b()
c()
```

---

## 38. Full Expression

以下各自构成 full expression：

- expression statement；
- initializer；
- assignment RHS；
- return expression；
- condition；
- function call；
- method call；
- match scrutinee。

未逃逸 temporary borrow 在 full expression 结束时终止。

---

## 39. Method Call 顺序

```text id="8ase05"
receiver.method(arg1, arg2)
```

顺序：

1. 求值 receiver；
2. 建立 receiver borrow / move；
3. 从左到右求其他 argument；
4. call。

无 two-phase borrow。

因此：

```text id="vwpqbt"
buf.set(
    buf.len - 1,
    0,
);
```

若 `set` 需要 `&mut self`，必须拒绝。

---

# 第八部分　整数运算

## 40. 普通运算

```text id="c3bo2f"
+
-
*
/
%
```

使用 checked semantics。

overflow → panic。

---

## 41. Wrapping

```text id="7qfjo4"
+%
-%
*%
```

---

## 42. Checked

```text id="1nzbwo"
+?
-?
*?
```

返回：

```text id="ff0pyu"
Option<T>
```

---

## 43. Division

signed division 向 0 截断。

```text id="5eebay"
-7_i32 / 3_i32 == -2
-7_i32 % 3_i32 == -1
```

除零与：

```text id="1fne6n"
MIN / -1
```

panic。

---

## 44. Shift

shift amount 必须小于 bit width。

否则 panic。

---

## 45. Unary Negation

普通 `-value` checked。

对 signed MIN 取负 panic。

---

# 第九部分　Operator

## 46. Maximal Munch

```text id="w4hc41"
+?
-?
*?
```

优先作为单 token。

postfix：

```text id="1dgw1j"
expr?
```

为 error propagation operator。

---

## 47. Precedence

从高到低：

```text id="enxesy"
postfix:
    call ()
    indexing []
    field .
    postfix ?

unary:
    ! ~ - & *

multiplicative:
    * / % *% *?

additive:
    + - +% -% +? -?

shift:
    << >>

bitwise and:
    &

bitwise xor:
    ^

bitwise or:
    |

comparison:
    < <= > >=

equality:
    == !=

logical and:
    &&

logical or:
    ||

range:
    .. ..=

assignment
```

因此：

```text id="fwwb7z"
x & MASK != 0
```

解释为：

```text id="gwqmlb"
(x & MASK) != 0
```

---

# 第十部分　数值转换

## 48. `cast<T>`

对 N-bit target：

```text id="wmwpz1"
cast<T>(x)
```

计算：

```text id="5jy7me"
x mod 2^N
```

再按 target signedness 解释。

---

## 49. `checked_cast<T>`

若数学值可由 target 表示：

```text id="sgnn0p"
Some(value)
```

否则：

```text id="wm7joc"
None
```

---

## 50. 没有 `bitcast`

本版本不存在通用 value-level bitcast。

representation reinterpretation 必须通过 raw machine boundary 明确完成。

---

# 第十一部分　聚合类型

## 51. Array

```text id="h80zkk"
[N]T
```

N 必须 const-evaluable。

---

## 52. Array Item Initializer

```text id="om63re"
[a, b, c]
```

从左到右求值。

---

## 53. Array Repeat

```text id="cx8p3n"
[expression; N]
```

要求 element type Copy。

expression 求值一次，再复制 N 次。

即使：

```text id="ax63px"
N == 0
```

expression 仍求值一次。

---

## 54. Index

```text id="hwfycr"
array[index]
```

越界 panic。

---

## 55. Slice

只读：

```text id="mz1w9u"
[]const T
```

可变：

```text id="gog513"
[]T
```

slice 不拥有底层 object。

---

## 56. Slice Copy

```text id="ps9vao"
[]const T
```

Copy。

```text id="hy47el"
[]T
```

non-Copy。

---

## 57. Slice Operations

```text id="zw7ecw"
slice.len
slice[index]
slice[a..b]
```

越界 panic。

---

## 58. Mutable Split

Core 不证明动态 range disjoint。

需要 disjoint mutable view 时使用：

```text id="ln1o2f"
split_at_mut
```

等 required-core helper。

---

## 59. Tuple

```text id="mxip2n"
(T1, T2, ...)
```

---

## 60. Struct

```text id="xc3t8m"
struct Task {
    pid: Pid,
    state: TaskState,
}
```

---

## 61. Newtype

```text id="y61c3w"
struct PhysAddr(usize);
```

创建 nominal type。

---

## 62. Enum

```text id="rkl4ml"
enum TaskState {
    Ready,
    Running,
    Blocked(*WaitQueue),
    Dead(ExitCode),
}
```

是 tagged union。

---

# 第十二部分　Pattern 与 Match

## 63. Exhaustiveness

enum match 必须穷尽。

---

## 64. By-value Match

Copy scrutinee 不被消费。

owned non-Copy scrutinee 被整体消费。

---

## 65. Payload Ownership

按值 match non-Copy enum 时，各 arm 获得对应 payload ownership。

---

## 66. Resource Match

```text id="tfw31l"
match maybe_frame {
    None => {
    }

    Some(frame) => {
        free_frame(frame);
    }
}
```

合法。

---

## 67. `_`

按值 wildcard 只能丢弃 discardable value。

```text id="r1ndz7"
Some(_)      // u32: OK
Some(_)      // Task: OK if discardable
Some(_)      // Frame: error
```

---

## 68. `let _ = value`

正式支持。

```text id="crcl41"
let _ = task;       // discardable -> OK
let _ = frame;      // resource -> error
```

---

## 69. Borrowed Match

```text id="cwbwrk"
match &maybe_frame {
    Some(frame) => {
        // frame: &Frame
    }

    None => {}
}
```

不会 move underlying resource。

---

## 70. Mutable Borrowed Match

```text id="8b2tov"
match &mut maybe_frame {
    Some(frame) => {
        // frame: &mut Frame
    }

    None => {}
}
```

---

## 71. Partial Move

不支持字段级 partial move 后继续使用 aggregate。

需要时整体 destructure。

---

# 第十三部分　Binding 与 Assignment

## 72. `let`

不可重新赋值。

仍可：

- read；
- move；
- borrow。

---

## 73. `var`

允许重新赋值与 mutable borrow。

---

## 74. Place

place 包括：

- mutable local；
- mutable field；
- array element；
- `&mut` target；
- raw dereference place。

---

## 75. Assignment 顺序

```text id="wntw0v"
place = expression;
```

执行：

1. 求 RHS；
2. 检查 LHS；
3. 结束旧 value lifetime；
4. 写入新 value；
5. place → Available。

---

## 76. Overwrite Discardable

old value discardable 时允许覆盖。

没有隐式 destructor。

---

## 77. Overwrite Resource

initialized resource-bearing value 不能普通覆盖。

```text id="1e2eco"
var frame =
    alloc_frame()?;

frame =
    alloc_frame()?;      // error
```

---

## 78. Reinitialize Moved Local

```text id="8j0uvf"
var frame =
    alloc_frame()?;

free_frame(frame);

frame =
    alloc_frame()?;
```

可以重新初始化。

仍需满足 defer rule。

---

# 第十四部分　`const` 与 `static`

## 79. `const`

`const` 是编译期 value。

type 必须 Copy。

---

## 80. Non-Copy Const

非法：

```text id="x2y69c"
const TASK: Task =
    Task { ... };
```

---

## 81. Borrowed Const

const 中 safe borrow 必须只来自 static storage。

---

## 82. `static`

```text id="yl06mm"
static VALUE: T =
    initializer;
```

是真实、唯一、具有 storage identity 的 object。

initializer 必须 const-evaluable。

---

## 83. Copy Static

Copy static 可按值读取。

---

## 84. Non-Copy Static

不能 safe move out。

可以 borrow。

---

## 85. Resource Static

resource-bearing static 是 program-lifetime owner。

不会发生 local scope-exit consumption。

---

## 86. `static mut`

普通访问都要求 unsafe。

---

# 第十五部分　控制流

## 87. `if`

condition 必须 bool。

可以产生 value。

---

## 88. `loop`

支持：

```text id="vulqis"
break
continue
```

---

## 89. `while`

普通条件循环。

---

## 90. 普通 `for`

支持：

- range；
- array borrow traversal；
- slice borrow traversal。

---

## 91. Slice `for`

```text id="y1bh9f"
[]const T
```

产生：

```text id="ypwhs8"
&T
```

```text id="117uwq"
[]T
```

产生：

```text id="ys8kgc"
&mut T
```

---

# 第十六部分　`for move`

## 92. Owned Array Traversal

```text id="vfm7j1"
for move item in array {
    ...
}
```

用于：

```text id="halgvw"
[N]T
```

每次获得 owned T。

---

## 93. Non-Copy Array

non-Copy array 被整个 move 入 traversal。

---

## 94. Resource Element

若 T resource-bearing，则正常路径必须处理全部 element。

---

## 95. `break`

resource element 尚有 remaining tail 时不能 `break`。

---

## 96. `return` / `?`

resource array traversal 中不能普通提前：

```text id="jh6j1h"
return
?
```

因为 Core 不追踪 remaining-tail ownership。

---

## 97. `continue`

当前 iteration resource 必须先消费或转移。

---

## 98. Discardable Element

若 T discardable，则提前退出时 remaining element 可以自然结束生命周期。

---

## 99. Panic

panic 不要求处理 remaining resource。

---

# 第十七部分　Copy / Move / Resource

## 100. 两个正交维度

```text id="mgnj1f"
Copyability:
    Copy
    non-Copy

Consumption:
    discardable
    must-consume
```

---

## 101. 内建 Copy

Copy：

```text id="b58uuf"
integers
bool
raw pointer
function pointer
&T
[]const T
```

non-Copy：

```text id="12mwix"
&mut T
[]T
MaybeUninit<T>
Atomic<T>
UnsafeCell<T>
resource-bearing
```

---

## 102. User Aggregate 默认 Non-Copy

普通 user-defined struct / enum 默认 non-Copy。

---

## 103. `@copy`

显式：

```text id="270cmc"
@copy
struct Point {
    x: i32,
    y: i32,
}
```

才允许结构性 Copy。

---

## 104. Generic `@copy`

```text id="zym3vt"
@copy
struct Pair<T, U> {
    first: T,
    second: U,
}
```

某个实例只有在实际成员都 Copy 时才 Copy。

---

## 105. Array / Tuple Copy

array / tuple 按 component structural Copy。

---

## 106. `Copy ⇒ discardable`

Copy 类型不能 resource-bearing。

---

## 107. Move

non-Copy value 按值赋值、传参或返回时发生 Move。

source 变为：

```text id="bcluj1"
Moved
```

---

## 108. `resource`

```text id="bneq9g"
resource struct Frame {
    number: usize,
}
```

表示：

```text id="sbwn76"
non-Copy
must-consume
```

---

## 109. Resource-bearing 传播

resource obligation 递归穿过：

```text id="8zeljp"
Option
Result
array
tuple
struct
enum
UnsafeCell
```

---

## 110. 正常退出

resource-bearing value 在正常路径必须：

- 消费；
- move 给其他 owner；
- 返回；
- `unsafe forget`。

---

## 111. Panic

panic path 不要求 resource cleanup。

---

## 112. `forget`

```text id="ad81we"
unsafe {
    forget(value);
}
```

消费 value，但不执行释放。

---

# 第十八部分　CFG Ownership

## 113. 状态

至少：

```text id="zvq2h1"
Available
Moved
```

---

## 114. Merge

正常 CFG join：

```text id="1wgbtr"
Available + Available -> Available
Moved + Moved         -> Moved
Available + Moved     -> error
```

---

## 115. `return` / `panic`

不再到达 merge 的 path 不参与 merge。

---

## 116. Loop Backedge

正常 backedge 上外层 binding state 必须与 loop entry 相同。

---

## 117. Loop Exit

所有正常到达同一 loop exit 的路径必须 state 一致。

---

# 第十九部分　`defer`

## 118. 基本语义

```text id="gxtj8f"
defer statement;
```

在当前 scope 正常退出时执行。

LIFO。

---

## 119. Late Evaluation

普通 expression value 在 defer 真正执行时求值。

---

## 120. Future-use Obligation

defer 使用某 binding 时，在所有会触发该 defer 的正常 exit edge 上，该 binding 必须满足 deferred operation 的状态要求。

包括：

```text id="iehw28"
scope end
return
?
break
continue
```

---

## 121. Fallible Reinit

以下非法：

```text id="844s83"
var frame =
    alloc_frame()?;

defer free_frame(frame);

free_frame(frame);

frame =
    alloc_frame()?;
```

因为最后一个 `?` 的 Err edge 会在 reinitialize 前触发 defer。

---

## 122. 合法模式

```text id="ubba76"
var frame =
    alloc_frame()?;

defer free_frame(frame);

let next =
    alloc_frame()?;

free_frame(frame);

frame = next;
```

---

## 123. Return 顺序

```text id="g3a1gs"
return expr;
```

执行：

1. 求 expr；
2. 保存 result；
3. scope exit；
4. defer；
5. return。

---

## 124. `?` 顺序

Err：

1. 保存 error；
2. scope exit；
3. defer；
4. return Err。

---

## 125. Defer Body 限制

defer 内不能通过：

```text id="75fiiu"
return
?
break
continue
```

发起非局部正常控制转移。

---

## 126. Panic

defer 中 panic 后剩余 defer 不保证执行。

---

# 第二十部分　Scope Exit 与 Borrow

## 127. Scope Exit Phase

正常 scope exit：

1. pending return/error/break value 已求值；
2. 结束不再被 pending defer 使用的 local borrow；
3. LIFO 执行 defer；
4. 每个 defer 后结束后续不再需要的 borrow；
5. 结束剩余 local；
6. 完成 control transfer。

---

## 128. Guard 模式

```text id="0zizqw"
let guard =
    lock.lock();

defer unlock(guard);

let data =
    guard.get_mut();

use(data);
```

若 `data` 不被 defer 使用，则其 borrow 在 `unlock(guard)` 前结束。

---

## 129. Defer 依赖 Borrow

如果：

```text id="jfvqis"
defer inspect(data);
```

则 data borrow 延长到该 defer 执行。

真实 LIFO 顺序必须合法。

---

# 第二十一部分　安全 Borrow

## 130. `&T`

shared borrow。

要求：

- non-null；
- aligned；
- initialized；
- valid；
- region 内持续有效。

Copy。

---

## 131. `&mut T`

exclusive mutable borrow。

non-Copy。

---

## 132. Shared Borrow 冲突

存在 `&T` 时 owner 不得：

- 普通写；
- move；
- 销毁；
- 建立冲突 `&mut T`。

---

## 133. Mutable Borrow 冲突

存在 `&mut T` 时 owner 不得：

- 直接读；
- 直接写；
- move；
- 销毁；
- 建立其他冲突 borrow。

---

## 134. Root-place 粒度

Core 以 root owned binding 为冲突粒度。

```text id="w46spx"
let pid =
    &task.pid;

task.state =
    Ready;      // error
```

---

## 135. Array Root 粒度

```text id="myxfhf"
let item =
    &array[i];

array[j] =
    value;      // error
```

不证明 `i != j`。

---

## 136. Named Borrow

存入 local binding 的 borrow 持续到 lexical scope exit phase。

不做一般 NLL shortening。

---

## 137. Temporary Borrow

未存入 binding / aggregate / return value 的 borrow 活到 full expression 结束。

---

## 138. Reborrow

已有 reference 可以建立 compatible temporary reborrow。

---

# 第二十二部分　Borrow-carrying Value

## 139. Borrow-carrying Type

递归包含：

```text id="fv10wf"
&T
&mut T
[]const T
[]T
```

的 type 是 borrow-carrying。

---

## 140. 允许的 Borrow-carrying Composition

允许：

```text id="aofqd7"
Option<&T>
Result<&T, E>

(&A, &B)

[4]&T

concrete non-generic struct
containing references/slices
```

---

## 141. Borrow Source Set

borrow-carrying value 概念上携带 source set。

```text id="on7n9w"
let pair =
    (&a, &b);
```

source set：

```text id="54ifjj"
{ a, b }
```

---

## 142. Copy Borrow-carrying Value

Copy aggregate 被复制后，新副本保留同样 source set。

---

## 143. Move Borrow-carrying Value

non-Copy borrow-carrying value move 后，source relationship 随 value 转移。

---

## 144. Local Assignment

borrow-carrying value 写入 local place时，其 source region 必须覆盖 destination local 的剩余 lexical region。

---

# 第二十三部分　Borrow-source Elision

## 145. 返回 Borrow 的 Source 属于接口

普通 function / method 返回 borrow-carrying value 时，source relation 属于函数接口。

---

## 146. Method

若 receiver 是：

```text id="652ivs"
&self
&mut self
```

且返回 borrow-carrying value：

> source 默认 = self。

其他 borrowed argument 不参与 source 选择。

---

## 147. Free Function：一个 Input Source

若 free function 恰有一个 top-level borrowed input，则 returned borrow source 为该 input。

```text id="lc4fhe"
fn head(
    items: []const T,
) -> &T
```

合法。

---

## 148. Free Function：零 Input Source

若没有 borrowed input，则 returned borrow 必须来自 static storage。

---

## 149. Free Function：多个 Input Source

若有多个 top-level borrowed input 且返回 borrow-carrying value：

> 签名非法。

```text id="3hfpg8"
fn choose(
    a: &T,
    b: &T,
) -> &T
```

非法。

---

## 150. 返回 Aggregate

多个 returned borrowed component 可以来自同一个 input source。

---

## 151. Static + Input

返回 aggregate 可以同时包含：

- 一个 input source；
- 任意 static borrow。

---

## 152. Function Pointer 限制

为了避免把 borrow-source ABI 扩展到间接调用：

> **KTL 0.4 function pointer 的返回类型不得 borrow-carrying。**

因此：

```text id="u4pg22"
fn(u32) -> bool
```

合法。

```text id="7khfqo"
fn(&Table) -> &Entry
```

作为普通 function pointer type 非法。

需要此类接口时使用：

- concrete method；
- concrete free function；
- raw pointer callback。

---

## 153. Extern 限制

`extern` function 返回类型不得是 safe borrow-carrying type。

FFI 应返回：

- value；
- raw pointer；
- handle。

不能直接返回 KTL safe reference。

---

## 154. Cursor 限制

method 返回 borrowed value时 source = 整个 `self`。

不支持：

```text id="n55zwb"
source = self.field
```

因此 cursor/parser 可能因为 returned view 冻结整个 cursor。

这是已知限制。

---

# 第二十四部分　Borrow Escape

## 155. Local Storage

non-static borrowed value 可以写入当前函数拥有的 local place，前提是 source region 足够长。

---

## 156. Non-local Storage

safe code 不得把带 non-static source 的 borrow-carrying value 写入：

- input `&mut` 可达 storage；
- `&mut self` 可达 storage；
- static；
- static mut；
- global；
- raw storage。

---

## 157. 例子

非法：

```text id="6luhla"
fn store(
    slot: &mut Option<&T>,
    value: &T,
) {
    *slot =
        Some(value);      // error
}
```

---

## 158. Raw Storage

unsafe code 可以把 borrowed value 放进 raw storage，但必须保证 source 在整个存储期间有效。

违反时 outside KTL specified semantics。

---

# 第二十五部分　Generics

## 159. Generic Parameter

```text id="g4zcp8"
struct Queue<T> {
    ...
}
```

```text id="xddmr8"
fn identity<T>(
    value: T,
) -> T {
    return value;
}
```

---

## 160. Generic Borrow Restriction

用户定义 generic parameter 不能实例化为 borrow-carrying type。

因此：

```text id="wyid5k"
Queue<u32>       // OK
Queue<Task>      // OK
Queue<Frame>     // OK
```

但：

```text id="arg5q9"
Queue<&Task>          // error
Queue<[]const u8>     // error
```

---

## 161. Generic Function 同样受限

```text id="qt6ytc"
identity<Task>(task)
```

可以。

```text id="doifvl"
identity<&Task>(&task)
```

非法。

如果需要 generic borrow：

```text id="t15iqv"
fn identity_ref<T>(
    value: &T,
) -> &T {
    return value;
}
```

---

## 162. Core Generic Exceptions

以下 compiler-known types可以携带 borrow-carrying argument：

```text id="e80y1x"
Option<T>
Result<T, E>
```

因为它们的 borrow propagation 由 Core 直接定义。

---

## 163. `MaybeUninit<T>`

`MaybeUninit<T>` 不允许 T 为 borrow-carrying。

---

## 164. `UnsafeCell<T>`

`UnsafeCell<T>` 不允许 T 为 borrow-carrying。

---

## 165. Generic Constraint

0.4 user-facing generic constraint 只有：

```text id="u40kkb"
T: Copy
```

`Copy` 是 compiler-known capability，不是 trait。

---

## 166. `T: Copy`

保证：

```text id="5lh17j"
T is Copy
T is discardable
T is not resource-bearing
```

由于 generic type argument 本身不能 borrow-carrying，因此也不会引入 generic borrow escape。

---

## 167. Generic Inherent Impl

允许：

```text id="s54vrb"
impl<T> Queue<T> {
    ...
}
```

以及：

```text id="auwerz"
impl<T: Copy> Buffer<T> {
    ...
}
```

---

## 168. 没有 User Trait

不存在：

```text id="f0jjvn"
trait Foo
impl Foo for Bar
```

接口复用依赖：

- modules；
- generics；
- function pointers；
- explicit ops records。

---

# 第二十六部分　Required Core Transfer Helpers

## 169. `replace`

```text id="zcst5x"
replace(
    place: &mut T,
    value: T,
) -> T
```

仅当：

```text id="j0zt2b"
T is not borrow-carrying
```

时可用。

语义：

1. old T 移出；
2. new T 写入；
3. 返回 old T。

因此可以安全替换：

```text id="ekq971"
Option<Frame>
```

但不支持：

```text id="p4j0v3"
Option<&T>
```

---

## 170. `swap`

```text id="t3tt48"
swap(
    a: &mut T,
    b: &mut T,
)
```

同样要求：

```text id="vg0w1k"
T is not borrow-carrying
```

这样 Core 不需要为 generic swap 实现 source-set transfer logic。

---

# 第二十七部分　函数与 Method

## 171. Function

parameter / return type 必须显式声明。

local 可以类型推导。

---

## 172. Method Receiver

只允许：

```text id="qhmc1v"
self
&self
&mut self
```

---

## 173. Function Pointer

```text id="0gcjam"
fn(u32) -> bool
```

是：

- Copy；
- non-null；
- no capture；
- no hidden allocation。

返回类型不得 borrow-carrying。

---

## 174. Unsafe Function Pointer

```text id="ps3be7"
unsafe fn(*u8) -> ()
```

调用要求 explicit unsafe block。

---

## 175. Extern Function

```text id="jst4c2"
extern abi(C)
fn memset(
    dst: *u8,
    value: i32,
    size: usize,
) -> *u8;
```

raw extern call 默认 unsafe。

extern return type不得 safe borrow-carrying。

---

## 176. `unsafe fn`

表示 caller 必须满足额外 contract。

---

## 177. Unsafe Function Body

`unsafe fn` body 不自动成为 unsafe context。

```text id="s4i5gi"
unsafe fn clear(
    ptr: *u32,
) {
    unsafe {
        *ptr = 0;
    }
}
```

---

# 第二十八部分　单一 Effect：`may_block`

## 178. Effect

Core 只跟踪：

```text id="3apd36"
may_block
```

---

## 179. Effect Declaration

```text id="ikxrsx"
fn wait()
    effects {
        may_block
    }
{
    ...
}
```

没有 effect clause：

```text id="ym5xyc"
{}
```

---

## 180. 普通 Call

必须满足：

```text id="45036h"
E_callee
    ⊆
E_caller
```

由于只有一个 effect，这等价于：

> 不带 `may_block` 的函数不能调用可能 block 的函数。

---

## 181. Function Pointer

function pointer 也携带是否 `may_block`。

```text id="u1jpix"
fn() -> ()
    effects {
        may_block
    }
```

无 block effect 的 function 可以赋给允许 block 的 pointer。

反向不行。

---

## 182. 移出 Core 的 Reliability Properties

以下不属于 function type：

```text id="6zbj5j"
may_alloc
may_fault
stack usage
recursion
trap residency
```

这些由 platform / verifier 分析。

---

# 第二十九部分　Raw Pointer Ownership

## 183. Raw Pointer

```text id="bbjtxu"
*T
*const T
```

可以：

- null；
- dangling；
- misaligned；
- 未映射；
- 指向 uninitialized storage。

---

## 184. Raw Dereference 是 Place

对普通 kernel raw pointer：

```text id="h9pr0g"
*ptr
```

可以用于：

- Copy read；
- non-Copy move-out；
- write；
- safe borrow construction。

---

## 185. Copy Raw Read

若 T Copy：

```text id="qwz2p3"
let x =
    unsafe {
        *ptr
    };
```

是 Copy read。

storage 保持 initialized。

---

## 186. Non-Copy Raw Move-out

若 T non-Copy：

```text id="9ku6qb"
let x =
    unsafe {
        *ptr
    };
```

是 ownership move-out。

之后 storage 为：

```text id="k4vz6i"
uninitialized
```

---

## 187. Move-out Contract

调用者必须保证：

- pointer valid；
- aligned；
- storage initialized；
- representation valid；
- 有唯一 move-out 权利；
- 没有冲突 safe borrow。

---

## 188. `*const T`

允许 Copy-read Copy T。

不能 move-out non-Copy T。

---

## 189. Raw Write

```text id="kotge3"
unsafe {
    *ptr = value;
}
```

如果 storage uninitialized，则初始化。

---

## 190. 覆盖 Discardable

initialized discardable T 可以 raw overwrite。

---

## 191. 覆盖 Resource

initialized resource-bearing T 不能 raw overwrite。

必须先 move-out 或明确保证 storage uninitialized。

---

## 192. Raw State 不由 Core 跟踪

Core 不静态维护 arbitrary raw storage 的 initialized/uninitialized typestate。

这是 unsafe contract。

---

# 第三十部分　Raw → Safe Borrow

## 193. Shared Borrow

```text id="70ds9t"
unsafe {
    let r =
        &*ptr;
}
```

构造：

```text id="g770zh"
&T
```

---

## 194. Shared Preconditions

必须保证：

- non-null；
- aligned；
- initialized；
- valid T；
- region 内持续有效；
- 无冲突 mutable access；
- address-space 可形成普通 safe reference。

---

## 195. Mutable Borrow

```text id="hm1tfy"
unsafe {
    let r =
        &mut *ptr;
}
```

构造：

```text id="2j4q06"
&mut T
```

额外要求 exclusive access。

---

## 196. `*const T` 不能构造 `&mut T`

严格禁止。

---

## 197. Safe World Re-entry

safe reference 构造完成后，普通 borrow checker重新接管。

---

## 198. Raw-origin Return

raw-origin safe borrow若被返回，仍须满足普通 borrow-source elision。

---

## 199. Static Raw-origin Return

无 borrowed input 的函数返回 safe borrow时，interface要求 static source。

例如：

```text id="0k93vu"
fn global_device()
    -> &Device
{
    return unsafe {
        &*DEVICE_PTR
    };
}
```

unsafe contract必须保证 target 为 program-lifetime storage。

---

# 第三十一部分　Typed Address Space

## 200. Raw Address Spaces

```text id="rmf66s"
*kernel T
*user T
*phys T
*mmio T
```

以及 const 变体。

省略 address-space：

```text id="cuwbu1"
*T
```

等价于：

```text id="9fzmyj"
*kernel T
```

---

## 201. `addr`

```text id="nup50j"
addr(pointer)
```

返回：

```text id="zqrhnr"
usize
```

机器地址。

safe，因为不访问 memory。

---

## 202. Integer → Pointer

```text id="8ojp6n"
unsafe {
    ptr<T>(address)
}
```

始终 unsafe。

---

## 203. `ptr_add`

```text id="5zox4m"
unsafe {
    ptr_add(
        pointer,
        count,
    )
}
```

地址增加：

```text id="ifdmy0"
count * sizeof(T)
```

---

## 204. `ptr_add_bytes`

字节级偏移。

---

## 205. User Pointer

```text id="1zactj"
*user T
```

不能 ordinary dereference。

通过：

```text id="ce1fwu"
copy_from_user
copy_to_user
```

等 platform API 访问。

---

## 206. Physical Pointer

```text id="378243"
*phys T
```

不能 dereference。

必须先映射。

---

## 207. MMIO Pointer

```text id="6kg9j9"
*mmio T
```

不能 ordinary load/store。

使用 MMIO intrinsic。

---

## 208. 没有 Typed Pointer Cast

没有单独：

```text id="wd58ak"
*A T -> *A U
```

操作。

当前使用：

```text id="gkztnv"
addr
+
ptr<U>
```

显式重建。

---

## 209. 没有 Raw Field Projection

当前 raw field address 使用：

```text id="5uz4ci"
addr(base)
+
offsetof(Type, field)
```

再构造对应 raw pointer。

是否需要 dedicated primitive 由 VirtIO reality test 决定。

---

# 第三十二部分　`MaybeUninit<T>`

## 210. 属性

```text id="hr7qmx"
MaybeUninit<T>
```

始终：

```text id="q5ms4j"
non-Copy
discardable
```

---

## 211. Generic Restriction

T 不得 borrow-carrying。

---

## 212. `uninit`

```text id="l9jrw0"
MaybeUninit::uninit()
```

是 const-evaluable intrinsic。

---

## 213. Safe `write`

只允许：

> T 非 resource-bearing。

```text id="grgq5t"
slot.write(value);
```

---

## 214. Resource T

```text id="80g3mb"
MaybeUninit<Frame>
```

类型可以存在。

safe `write(Frame)` 不允许。

需要 unsafe raw initialization或 owning slot abstraction。

---

## 215. `assume_init`

```text id="v5xfzb"
unsafe fn assume_init(
    self,
) -> T
```

消费整个 MaybeUninit。

---

# 第三十三部分　`UnsafeCell<T>`

## 216. 定义

```text id="jmvl84"
UnsafeCell<T>
```

允许在 shared-reference abstraction 背后保存可变 storage。

---

## 217. Generic Restriction

T 不得 borrow-carrying。

---

## 218. Copy

永远 non-Copy。

---

## 219. Resource

resource-bearing 属性继承 T。

---

## 220. `get`

```text id="gr2z0b"
fn get(
    &self,
) -> *T
```

返回 mutable raw pointer。

实际 dereference仍 unsafe。

---

# 第三十四部分　Atomic

## 221. `Atomic<T>`

Atomic：

- 具有 storage identity；
- 永远 non-Copy；
- 是 Core interior-mutability primitive。

---

## 222. Plain Access 禁止

不能用 ordinary load/store 绕过 Atomic API。

---

## 223. Core Atomic 类型

至少：

```text id="52eqz4"
Atomic<u8>
Atomic<u16>
Atomic<u32>
Atomic<u64>
Atomic<usize>

Atomic<*T>
Atomic<*const T>
```

---

## 224. Target 实现

语言定义 atomic semantics，不要求单条 ISA instruction。

RV64 backend 可以用 wider LR/SC 实现窄 atomic。

---

# 第三十五部分　Atomic Memory Model

## 225. 基本关系

定义：

```text id="qrxmi3"
sequenced-before
modification order
release sequence
synchronizes-with
happens-before
```

采用 C++11/Rust-style acquire-release model。

---

## 226. Modification Order

每个 atomic object 的 modification 存在唯一 total order。

---

## 227. Release Sequence

Release 或更强 write/RMW 后，同一 atomic object modification order 中连续 RMW 构成 release sequence。

---

## 228. Synchronizes-with

Acquire 观察到 Release 或其 release-sequence value 时：

```text id="0gljhl"
Release
    synchronizes-with
Acquire
```

---

## 229. Happens-before

由：

- sequenced-before；
- synchronizes-with；

及传递关系形成。

---

## 230. `Relaxed`

只保证：

- atomicity；
- modification order。

---

## 231. `Release`

Release 之前 sequenced-before 的 operation 可以经匹配 Acquire 对另一 execution context 可见。

---

## 232. `Acquire`

匹配 Release 后：

```text id="i4f1y7"
before Release
    happens-before
after Acquire
```

---

## 233. `AcqRel`

同时具有 Acquire + Release。

---

## 234. `SeqCst`

参加统一 sequentially-consistent order。

---

## 235. Data Race

unsafe abstraction 若制造未同步 conflicting ordinary memory access，则违反 unsafe contract。

---

# 第三十六部分　Atomic API

## 236. Integer Atomic

支持：

```text id="yuk6dd"
load
store
exchange
compare_exchange

fetch_add
fetch_sub
fetch_and
fetch_or
```

---

## 237. Pointer Atomic

支持：

```text id="9nk35h"
load
store
exchange
compare_exchange
```

不支持 integer fetch arithmetic。

---

## 238. Load Ordering

```text id="801n0r"
Relaxed
Acquire
SeqCst
```

---

## 239. Store Ordering

```text id="2xsbba"
Relaxed
Release
SeqCst
```

---

## 240. RMW Ordering

```text id="pkk398"
Relaxed
Acquire
Release
AcqRel
SeqCst
```

---

## 241. `compare_exchange`

```text id="arbji3"
compare_exchange(
    expected,
    desired,
    success_order,
    failure_order,
) -> Result<T, T>
```

---

## 242. Failure Ordering

合法矩阵：

| Success | Failure |
|---|---|
| `Relaxed` | `Relaxed` |
| `Acquire` | `Relaxed`, `Acquire` |
| `Release` | `Relaxed` |
| `AcqRel` | `Relaxed`, `Acquire` |
| `SeqCst` | `Relaxed`, `Acquire`, `SeqCst` |

其他组合非法。

---

## 243. 没有 Standalone Atomic Fence

本版本不定义：

```text id="upzsnt"
atomic_fence
compiler_fence
```

---

# 第三十七部分　Volatile 与 MMIO

## 244. Volatile Scalar

compiler-known closed set：

```text id="33uoyy"
u8
u16
u32
u64
usize

i8
i16
i32
i64
isize
```

不允许 aggregate / resource。

---

## 245. Kernel Volatile Load

```text id="obxwem"
unsafe fn volatile_load(
    ptr: *kernel const T,
) -> T
```

其中 T 必须属于 VolatileScalar closed set。

---

## 246. Kernel Volatile Store

```text id="go7xgk"
unsafe fn volatile_store(
    ptr: *kernel T,
    value: T,
)
```

---

## 247. MMIO Load

```text id="y42018"
unsafe fn mmio_load(
    ptr: *mmio const T,
) -> T
```

---

## 248. MMIO Store

```text id="52v9sc"
unsafe fn mmio_store(
    ptr: *mmio T,
    value: T,
)
```

---

## 249. Volatile 保证

只保证该 machine access 实际发生。

不保证：

- atomicity；
- CPU synchronization；
- device ordering；
- pointer validity。

---

## 250. Device Fence

设备 ordering 由 Architecture Supplement：

```text id="idtbkw"
arch::device_read_fence()
arch::device_write_fence()
```

定义。

---

# 第三十八部分　Layout 与 ABI

## 251. Layout Query

```text id="uplzoc"
sizeof(T)
alignof(T)
offsetof(T, field)
```

---

## 252. 默认 Layout

user aggregate 默认 layout：

- implementation-defined；
- 非 stable ABI。

---

## 253. `@repr(C)`

使用 target C ABI layout。

---

## 254. `@align`

```text id="rvvmn0"
@align(4096)
```

指定 minimum alignment。

---

## 255. `@packed`

packed field 可能未自然对齐。

按值访问可以。

不能形成 misaligned safe reference。

---

## 256. `@export`

```text id="lwbcra"
@export("_start")
```

指定 external symbol。

---

## 257. `@section`

```text id="76976i"
@section(".text.boot")
```

指定 linker section。

可用于 function / static。

---

# 第三十九部分　Const Evaluation

## 258. Const Context

必须 const-evaluable：

- const initializer；
- static initializer；
- array length；
- static_assert；
- constant attribute argument。

---

## 259. 允许的 Const Expression

允许：

```text id="uc8h6e"
integer / bool / string literal

integer arithmetic
bitwise operation
comparison
logical operation

if
match

aggregate construction
array construction

cast
checked_cast

sizeof
alignof
offsetof

Atomic::new
UnsafeCell::new
MaybeUninit::uninit
```

---

## 260. 不允许 Function Call

除规定 intrinsic constructor 外：

> const expression 不允许普通函数调用。

因为本版本没有 `const fn`。

---

## 261. 不允许 Const Mutation / Loop

禁止：

```text id="2y4jx4"
var
assignment

for
while
loop
defer
```

---

## 262. 不允许 Machine Operation

const evaluation 禁止：

```text id="7thqt4"
raw dereference
raw→safe borrow

Atomic operation
UnsafeCell mutation

volatile
MMIO

FFI
asm

static mut access
```

---

## 263. Const Panic

若 compile-time evaluation 执行到 panic：

> compile-time error。

---

## 264. `static_assert`

```text id="ctq766"
static_assert(
    sizeof(Pte) == 8
);
```

condition 必须：

- const-evaluable；
- bool；
- true。

---

# 第四十部分　`Option`、`Result`、`?`

## 265. `Option<T>`

Required core 概念定义：

```text id="1u0imr"
@copy
enum Option<T> {
    Some(T),
    None,
}
```

是 compiler-recognized borrow-transparent generic。

---

## 266. `Result<T, E>`

```text id="jvrbwu"
@copy
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

同样可以携带 borrow-carrying argument。

---

## 267. `?`

```text id="2cmw6p"
expression?
```

仅用于 Result。

Ok → value。

Err → 正常 error return，执行 scope-exit / defer。

---

## 268. Error Type

传播 error type 必须与当前 function error type 完全相同。

无隐式 error conversion。

---

# 第四十一部分　Panic

## 269. Panic

```text id="4jix0t"
panic("message");
```

类型：

```text id="g8927l"
!
```

---

## 270. 不 Unwind

panic：

- 不 exception unwind；
- 不保证 defer；
- 不保证 resource cleanup；
- 不允许 catch。

---

## 271. Panic Handler

freestanding target 可提供：

```text id="gotyvr"
fn panic_handler(
    info: &PanicInfo,
) -> !
```

---

# 第四十二部分　Unsafe Contract

## 272. Unsafe Function

```text id="1ib9kr"
unsafe fn foo(...) {
    ...
}
```

表示 caller contract。

---

## 273. Unsafe Function Body

不会自动成为 unsafe context。

具体 operation 仍必须：

```text id="t8eze6"
unsafe {
    ...
}
```

---

## 274. Contract Violation

若实际执行 unsafe operation 时违反其前置条件：

> 从该点起 outside KTL specified semantics。

---

# 第四十三部分　单一 Blocking Effect

## 275. `may_block`

Core 唯一 tracked effect：

```text id="bb36ho"
may_block
```

---

## 276. Declaration

```text id="eyiq1e"
fn wait()
    effects {
        may_block
    }
{
    ...
}
```

---

## 277. Ordinary Call

不允许：

```text id="wp1ley"
caller {}
    calls
callee { may_block }
```

---

## 278. Function Pointer

function pointer type可以携带：

```text id="93amie"
effects { may_block }
```

---

## 279. Verifier Property

以下不属于 Core effect：

```text id="gmbo60"
allocates
may fault
stack usage
recursion
trap-resident dependency
```

---

# 第四十四部分　Kernel Context

## 280. Context Kind

Core 识别：

```text id="u2fwau"
Normal
IRQ
TrapEntry
Trap
```

---

## 281. `@irq`

```text id="m0pk2l"
@irq
fn timer_irq(...) {
    ...
}
```

不得具有或调用：

```text id="tdkzjs"
may_block
```

---

## 282. `@trap_entry`

表示 minimal trap context。

不得：

```text id="bm1uyq"
may_block
```

---

## 283. `@trap`

表示 ordinary trap handler context。

是否允许 `may_block` 由 Architecture Supplement 明确规定。

---

## 284. 特殊 Context 不能普通进入

Normal / TrapEntry 等不同 special context 之间不能依靠普通 call 任意跳转。

---

# 第四十五部分　Trap Context Transition

## 285. 标准路径

```text id="mygjrl"
hardware trap
      ↓
architecture stub
      ↓
@trap_entry
      ↓
context transition
      ↓
@trap
      ↓
trap return
```

---

## 286. `arch::enter_trap_handler`

Architecture Supplement 定义特殊 intrinsic：

```text id="4j6mxp"
arch::enter_trap_handler(
    frame,
    named_handler,
) -> !
```

---

## 287. 静态限制

Core 必须检查：

1. 只能出现在 `@trap_entry`；
2. tail position；
3. 每条正常 path 至多一次；
4. target 必须是 compile-time known named `@trap` function；
5. 不能传 runtime function pointer；
6. result type `!`；
7. handler `may_block` 必须满足 ordinary trap context contract。

---

## 288. 不是 Effect Hole

该 intrinsic 是 machine-context control transfer，不是 ordinary function call。

只能在规定 context 使用。

---

## 289. Trap Nesting

Architecture runtime 负责：

- nesting enter；
- ordinary handler transition；
- trap return；
- nesting exit。

---

# 第四十六部分　Architecture Boundary

## 290. Entry Stub

进入 `@trap_entry` 前至少建立：

- trap stack；
- register save；
- TrapFrame；
- KTL ABI。

---

## 291. Inline ASM

具体语法由 Architecture Supplement 定义。

Core 只规定：

- asm 是 unsafe；
- `unsafe fn` 中仍需 unsafe block；
- clobber / ABI 必须准确；
- 若 asm 会 block，enclosing function 必须声明 `may_block`。

---

## 292. Verifier 与 Core 分离

stack bound、recursion、trap residency 等不改变语言程序语义。

它们由独立 verification layer 处理。

---

# 第四十七部分　Conformance Suite

## 293. 目录

```text id="ewpnux"
tests/conformance/
├── accept/
└── reject/
```

---

## 294. Reject Diagnostic

每个 reject test 必须声明 expected diagnostic class。

例如：

```text id="xk0q6x"
//@ reject: E_RESOURCE_OVERWRITE
```

只有：

1. reject；
2. primary category 匹配；

才算通过。

---

## 295. Basic Type / Sized

至少：

```text id="7lp34b"
signed_min_literal_accept
positive_signed_overflow_literal_reject

recursive_struct_by_value_reject
recursive_enum_by_value_reject
recursive_pointer_accept
```

---

## 296. Arithmetic

```text id="th5kw8"
checked_overflow_panics
wrapping_overflow_accept
checked_overflow_none

division_by_zero_panics
signed_division_toward_zero
shift_out_of_range_panics

bitwise_precedence_accept
maximal_munch_checked_operator
```

---

## 297. Copy / Move

```text id="lq8xpu"
copy_opt_in_accept
default_struct_duplicate_reject

move_noncopy_accept
use_after_move_reject
```

---

## 298. Resource

```text id="lfwyl6"
resource_wrapper_propagates
resource_scope_leak_reject

resource_overwrite_reject
resource_reinit_accept

resource_match_accept
resource_wildcard_reject

forget_resource_accept
```

---

## 299. CFG

```text id="0541oy"
cfg_mixed_move_reject
cfg_both_moved_accept

cfg_return_branch_not_joined_accept
cfg_panic_branch_not_joined_accept

loop_backedge_move_reject
loop_exit_mismatch_reject
```

---

## 300. Array

```text id="bwfe5t"
array_repeat_copy_accept
array_repeat_noncopy_reject

array_oob_panics
slice_oob_panics
```

---

## 301. `for move`

```text id="d4gruv"
for_move_resource_accept

for_move_resource_break_reject
for_move_resource_return_reject
for_move_resource_question_reject

for_move_continue_consumed_accept
for_move_continue_unconsumed_reject

for_move_discardable_break_accept
```

---

## 302. Defer

```text id="3h1j27"
defer_lifo_accept
defer_late_eval_accept

defer_return_value_before_cleanup

defer_moved_exit_reject
defer_fallible_reinit_reject
defer_safe_reinit_accept

defer_guard_named_borrow_accept
defer_borrow_after_owner_consume_reject
```

---

## 303. Borrow

```text id="u2htlf"
temporary_borrow_accept
named_borrow_modify_reject

shared_borrow_move_reject
shared_borrow_mutate_reject

mutable_borrow_owner_read_reject
mutable_borrow_conflict_reject

root_field_borrow_conflict_reject
array_root_borrow_conflict_reject

sequential_reborrow_accept
```

---

## 304. Borrow-carrying Value

```text id="p6t2yu"
option_reference_accept
tuple_multiple_borrow_source_accept

borrow_carrying_copy_preserves_source
borrow_carrying_move_preserves_source

local_borrow_storage_accept
external_borrow_storage_reject
static_borrow_escape_reject
```

---

## 305. Borrow-source Elision

```text id="l67zcx"
method_return_self_source_accept
method_return_other_parameter_reject

free_single_input_return_accept
free_zero_input_static_return_accept
free_multi_input_return_reject

return_same_source_pair_accept

cursor_second_call_reject
```

---

## 306. Function Pointer Restriction

```text id="d6mho4"
fn_pointer_plain_return_accept
fn_pointer_borrow_return_reject

extern_raw_pointer_return_accept
extern_safe_borrow_return_reject
```

---

## 307. Generics

```text id="y7eczo"
generic_value_u32_accept
generic_value_task_accept
generic_value_frame_accept

generic_borrow_argument_reject
generic_slice_argument_reject

generic_reference_parameter_accept

generic_inherent_impl_accept
generic_copy_bound_accept
```

---

## 308. No Trait

parser / resolver 必须拒绝：

```text id="j3jnlm"
trait
impl Trait for ...
```

作为 0.4 language syntax。

---

## 309. Core Generic Exceptions

```text id="lt0i1q"
option_reference_accept
result_reference_accept

user_generic_reference_instantiation_reject
```

---

## 310. `replace` / `swap`

```text id="lms3b9"
replace_task_accept
replace_resource_option_accept

replace_borrow_carrying_reject
swap_borrow_carrying_reject
```

---

## 311. Raw Ownership

```text id="viz0q2"
raw_copy_read_accept
raw_noncopy_move_accept

raw_const_copy_accept
raw_const_move_reject

raw_uninitialized_write_accept
```

runtime/contract test：

```text id="wklswt"
raw_double_move_violation
raw_resource_overwrite_violation
```

---

## 312. Raw → Safe

```text id="zo9cc2"
raw_shared_borrow_accept
raw_mut_borrow_accept
raw_const_to_mut_reject

user_raw_to_safe_reject
phys_raw_to_safe_reject
mmio_raw_to_safe_reject

raw_static_return_accept
```

---

## 313. Address Space

```text id="kukhg3"
kernel_deref_requires_unsafe

user_plain_deref_reject
phys_plain_deref_reject
mmio_plain_deref_reject

implicit_address_space_conversion_reject
```

---

## 314. MaybeUninit

```text id="uho3ol"
maybeuninit_noncopy

maybeuninit_task_write_accept
maybeuninit_resource_safe_write_reject

maybeuninit_assume_init_consumes
maybeuninit_double_extract_reject

maybeuninit_borrow_type_reject
```

---

## 315. UnsafeCell

```text id="4f31lr"
unsafe_cell_noncopy
unsafe_cell_resource_propagates

unsafe_cell_borrow_type_reject

unsafe_cell_get_accept
```

---

## 316. Atomic

```text id="gx0qgs"
atomic_load_release_reject
atomic_store_acquire_reject
atomic_rmw_acqrel_accept

atomic_pointer_fetch_add_reject

atomic_invalid_failure_order_reject
```

---

## 317. Volatile / MMIO

```text id="3ab818"
volatile_kernel_scalar_accept
mmio_scalar_accept

volatile_aggregate_reject
volatile_resource_reject

mmio_plain_deref_reject
```

---

## 318. Const

```text id="kb7w0z"
const_copy_accept
const_noncopy_reject

static_noncopy_borrow_accept
static_noncopy_move_reject

const_loop_reject
const_function_call_reject
const_panic_reject

static_assert_accept
static_assert_false_reject
```

---

## 319. Effect

```text id="wk2bdq"
may_block_direct_missing_reject

blocking_function_pointer_accept
blocking_effect_erasure_reject

irq_block_reject
trap_entry_block_reject
```

---

## 320. Trap Transition

```text id="23notr"
trap_transition_accept

trap_direct_special_call_reject

trap_transition_non_tail_reject
trap_transition_twice_reject
trap_transition_dynamic_handler_reject
trap_transition_nontrap_handler_reject
```

---

## 321. Removed Feature Tests

必须确认以下语法不属于 0.4：

```text id="uxs88p"
pin_type_reject
trait_declaration_reject
const_fn_reject
bitcast_reject
```

---

# 第四十八部分　Reality Tests

## 322. `frame_allocator.kt`

验证：

- resource；
- raw storage；
- move-out；
- arrays；
- fallible cleanup；
- defer。

---

## 323. `spinlock.kt`

验证：

- Atomic；
- Acquire / Release；
- UnsafeCell；
- raw→safe borrow；
- guard resource；
- defer；
- generic payload。

不依赖 Pin。

---

## 324. `page_table_map.kt`

验证：

- Frame ownership；
- physical pointer；
- fallible multi-step mapping；
- rollback ergonomics；
- success commit。

用于判断：

```text id="wbe8q9"
errdefer
```

是否真的值得加入 0.5。

---

## 325. `dtb_walker.kt`

验证：

- concrete borrow-carrying view；
- method return source=self；
- parser/cursor usability。

用于判断：

```text id="sswscn"
borrow-source projection
```

是否必要。

---

## 326. `virtio_mmio_regs.kt`

验证：

- typed MMIO；
- mmio_load/store；
- device fence；
- `offsetof`；
- raw field address ergonomics。

---

## 327. `intrusive_node.kt`

不再验证语言级 Pin。

改为验证：

- stable owner library；
- raw self-link；
- object identity；
- unsafe invariant containment。

如果事实证明没有语言级 Pin 就无法提供合理 safe abstraction，再重新讨论。

---

# 第四十九部分　已知限制

## 328. Resource Rollback

当前没有：

```text id="wfz2e9"
errdefer
```

fallible resource commit 需要显式 match cleanup。

---

## 329. Generic Discard

没有：

```text id="9xqyi1"
Discardable
```

constraint。

---

## 330. Const Repeat

没有：

```text id="h6wzrx"
[const EXPR; N]
```

因此 non-Copy static arrays 初始化可能笨重。

---

## 331. Borrowed User Generic

不支持：

```text id="y5i5so"
Queue<&T>
Map<K, &V>
```

这是有意减法。

---

## 332. Cursor Borrow

method borrowed return 默认依附整个 `self`。

可能阻止：

```text id="rd9um6"
let a = cursor.next();
let b = cursor.next();
```

同时保存 view。

---

## 333. No Pin

长期地址稳定由 owner/library invariant 保证。

---

## 334. No Trait

接口需要：

- module；
- generics；
- function pointer；
- explicit ops struct。

---

## 335. No `const fn`

复杂编译期计算需要直接写 const expression 或预生成数据。

---

## 336. Pointer Ergonomics

没有：

```text id="nllb84"
ptr_cast
ptr_offset(isize)
raw_field_ptr
```

---

## 337. No Send / Sync

没有语言级 cross-hart ownership marker。

是否需要由 safe concurrency API 的真实设计决定。

---

# 第五十部分　0.5 Backlog（非规范）

## 338. 只有 Reality Test 可以打开这些议题

候选：

```text id="3p3ruf"
errdefer

Discardable

[const EXPR; N]

borrow-source projection

language-level Pin

user-defined Trait

const fn

ptr_offset(isize)
typed pointer cast
raw field pointer

Send / Sync-like marker

standalone atomic fence
```

---

## 339. 不再预先设计

某项能力只有在真实代码满足至少一项时才重新讨论：

- 现有 Core 无法表达；
- safe abstraction 被迫向 caller 泄漏大量 unsafe；
- 出现大量机械重复；
- 教学模型明显恶化；
- library abstraction 无法解决。

---

# 第五十一部分　Compiler Model

## 340. 第一版 Compiler Pass

建议：

```text id="u779i8"
lex / parse
        ↓
name resolve
        ↓
type + generic
        ↓
Sized / layout
        ↓
Copy / resource classification
        ↓
local CFG Move analysis
        ↓
defer exit-edge analysis
        ↓
lexical root-place borrow analysis
        ↓
borrow source / escape check
        ↓
may_block check
        ↓
const / unsafe / address-space check
        ↓
target lowering
        ↓
RV64 codegen
```

---

## 341. 第一版不需要

```text id="vgq8q3"
trait solver
lifetime syntax
general region solver
NLL
Pin checker
const function interpreter
general bit-validity engine
whole-program ownership proof
general raw typestate
```

这正是本轮 Feature Purge 的主要目标。

---

# 第五十二部分　Freeze 条件

## 342. 当前状态

> **KTL 0.4.1-rc3 — Lean Core Candidate**

尚未重新宣布 Semantic Freeze。

---

## 343. 冻结前规范条件

至少要求：

1. 当前文档无已知内部矛盾；
2. conformance suite 覆盖关键规则；
3. reject case 有稳定 diagnostic category；
4. RV64 Architecture Supplement 最低骨架存在。

---

## 344. 冻结前实现条件

至少存在：

```text id="mc1x6m"
ktlc --check
```

原型，能够完成：

- parse / resolve；
- type check；
- Sized；
- simple generic；
- Copy/resource；
- CFG move；
- defer exit-edge；
- lexical/root-place borrow；
- borrow-source / escape；
- `may_block`；
- const subset；
- unsafe/address-space。

---

## 345. 冻结前 Reality Tests

至少完成：

```text id="z9nm2b"
frame_allocator.kt
spinlock.kt
page_table_map.kt
dtb_walker.kt
virtio_mmio_regs.kt
intrusive_node.kt
```

---

## 346. 冻结判断

如果上述代码能够：

- 自然表达；
- unsafe boundary 局部；
- 不迫使重新加入被删掉的机制；
- checker 仍保持小而局部；

则 0.4 可以正式进入：

> **Core Semantic Freeze**

---

# 第五十三部分　最终程序员心智模型

## 347. 类型的两个轴

```text id="r3pauf"
              Type
                │
       ┌────────┴────────┐
       │                 │
  Copyability        Consumption
       │                 │
 Copy / non-Copy   discard / must-consume
```

---

## 348. Owned State

```text id="zd0j0w"
Available
SharedBorrowed
MutBorrowed
Moved
```

---

## 349. Borrow

```text id="sfbpch"
temporary
    → full expression

named
    → lexical scope

returned borrow
    → one-source elision

non-static borrow
    → cannot escape into longer-lived storage
```

没有：

```text id="nc5kdx"
lifetime syntax
NLL
general region inference
```

---

## 350. Resource

```text id="20h9un"
resource
    ↓
explicit consume
or
explicit ownership transfer
```

没有隐式 destructor。

---

## 351. Raw Boundary

```text id="foxpxi"
initialized raw storage
        │
        ├── Copy read
        │
        └── non-Copy move-out
                 ↓
            uninitialized

uninitialized
        │
        └── raw write
                 ↓
            initialized
```

---

## 352. Safe Machine Boundary

```text id="7e3xqz"
raw pointer
    ↓ unsafe establishes invariants
&T / &mut T
    ↓
ordinary borrow checker
```

---

## 353. Concurrency Foundation

只有两个基础：

```text id="gp15yg"
Atomic<T>
UnsafeCell<T>
```

没有更大的 interior-mutability system。

---

## 354. 地址空间

```text id="znwx6u"
*kernel
*user
*phys
*mmio
```

是不同 raw-pointer type。

---

## 355. Kernel Context

Core 只回答一个简单问题：

> 这条调用链会不会 block？

通过：

```text id="kjy05f"
may_block
```

处理。

更强的 trap reliability 问题留给 verifier。

---

# 结语

Feature Purge 之后，KTL 0.4 的身份重新变得清楚。

它不再试图成为：

> 一个去掉显式 lifetime syntax 的小 Rust。

而更接近：

> **一门采用 Oberon 式克制方法重新设计的现代内核语言。**

它保留真正构成 KTL 身份的部分：

```text id="iecc1r"
Copy × Consumption
        +
resource-bearing propagation
        +
local CFG Move checking
        +
conservative lexical borrow
        +
raw storage ownership
        +
typed address spaces
        +
UnsafeCell / Atomic
        +
explicit machine boundary
```

而把：

```text id="yc2y0v"
Trait
Pin
const fn
general bitcast
large effect system
generic lifetime machinery
trap verifier semantics
```

全部从第一版语言核心移出。

从 rc3 开始，再增加任何一个 Core mechanism，都应该回答一个具体问题：

> **没有它，哪一段真实内核代码写不出来？**

回答不了，就不加。

下一步已经不是继续设计 KTL。

下一步是：

```text id="vyzh6n"
tests/conformance
        ↓
ktlc --check
        ↓
frame_allocator.kt
        ↓
spinlock.kt
        ↓
page_table_map.kt
        ↓
dtb_walker.kt
        ↓
virtio_mmio_regs.kt
```

然后看现实是否要求我们把任何被删除的东西请回来。