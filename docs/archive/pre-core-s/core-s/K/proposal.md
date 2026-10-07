# K：Core S 内核与 RV64 目标接口（候选 r1）

仅设计；依赖已通过的 R r1（`d9c95d3d804f55a2bb46959d4f831f8bc0bfb218167e25511e144c68d06a32de`）。本文件不能证明编译器已接受示例。

## 1. 目标与破坏性边界

设计名 `rv64-core-s-v1`，不是 `rv64-core-v1` 的源兼容别名。继承 [旧目标](../../../../docs/architecture-rv64.md) R1 的 ELF64 小端、RV64IMA+Zicsr+Zifencei、S-mode、soft-float LP64、静态链接、16 字节调用栈对齐、平台 `_start`、节及链接重定位规则；继承固定 psABI 提交及旧 R5/R6 的运行时物理 ABI、TrapFrame 和无调度陷阱合同。不增加 ISA、抢占或平台能力。
改动源合同：所有普通值复制；无引用、隐式移动、Copy/resource、自动析构、方法或用户泛型；删除 MaybeUninit/UnsafeCell/Atomic 包装器、MemoryOrder、窄/usize/指针原子实例、packed/newtype/tuple 布局及 C 聚合按值传参。固定切片和枚举仍可存在。
存储初始化、别名、同步、设备协议及句柄责任由实际访问合同维护；Core 检查类型、位置和 unsafe 授权，不保证唯一释放、不泄漏或无悬垂。不能继续宣称旧 Borrow/Ownership 的引用/移动保证自动适用。

## 2. 存储、指针与切片

`*kernel T` / `*kernel const T` 是内核虚拟地址；`user`、`phys`、`mmio` 保留相同拼写和 64 位地址表示。只允许 kernel 普通解引用；user 必须由显式平台函数进行故障可恢复的用户拷贝，phys 必须由平台建立映射，mmio 必须用本文件内建。重新标记地址不能替代这些步骤。
原始地址可以为零、未映射、未对齐或悬空；形成/复制地址不访问内存。实际读写须非空、自然对齐、完整范围存活且有映射和读写权限；读要求初始化及合法表示，写不要求旧内容合法。零大小访问仍要求非空、对齐和存活的逻辑对象身份，不访问字节。
`*p`（仅 kernel）形成位置，读取复制 T，写入复制新 T；两者 unsafe。const 指针禁止写入。覆盖旧值不释放资源；读取不使源未初始化。指针目标之间不自动产生 noalias/restrict 或按类型不别名假设。
原始存储可以是平台分配的字节区或具体类型的未初始化槽；不需要包装器。对齐、范围、初始化事实均独立于指针类型。`var slot: T;` 分配 T 大小/对齐的槽，不隐式零填充；只能在完整初始化后读取。任意 raw 写入的初始化事实不由 Core 证明。
也可定义 `@align(4096) struct PageBytes { bytes: [4096]u8 }`；合法字节区允许通过数字地址重新解释成具体对象，访问仍须逐次满足完整合同。字节填零不自动证明任意 T 合法；尤其函数指针零值和非法枚举标签不得读取。
字段取址不读基址指向的聚合，可逐字段写入未初始化结构；数组元素同理。所有字段/元素完成合法初始化后才能读整个聚合，填充除外。不能通过未初始化枚举载荷投影初始化：枚举用完整变体值写入；只有合法活动标签的载荷可投影。
填充及未激活枚举载荷可含任意字节；聚合复制可搬运这些字节，但不能将它们当用户字段读取。字节复制可以搬运未初始化字节并保留其未初始化事实，不把它们变为合法值。
切片是复制的地址/长度描述符，不是语言引用。`[]T` 可写，`[]const T` 只读；二者只描述 kernel 普通 RAM。形成切片要求非空对齐地址、`len*sizeof(T)` 可由 usize 表示、完整存活范围及对应权限，全部元素是合法已初始化 T；空切片可用对齐哨兵或一过尾地址，无元素访问权。后续访问仍须存活及同步。
切片下标检查失败 panic；通过切片读/写元素是 unsafe，防止用安全下标绕过原始存储合同。`.len` 与 `.data` 只读描述符，分别为 usize 与相应 const 性的 kernel 指针；不访问元素。复制可写切片不授予独占，也不复制元素。
从局部/静态取址不延长寿命；保存一个稍后失效的指针不等于已经进行无效访问。执行非法读取、竞争或无效机器操作从该操作起超出规定语义；检查失败仍是 panic。

## 3. 有限内建族及已知类型规则

下表的 T/P 是文档元变量，不是用户可声明的泛型。内建有保留声明身份、不能取函数指针。T 必须是具体有限大小对象类型；由第一个已知指针/位置操作数直接选择，其余操作数须与该 T 完全相符。不反推未知类型，不查找用户操作，不重放用户代码。所有普通值实参从左到右各求值一次并保存，随后执行操作；取址实参按位置规则计算一次。布局操作数不求值为普通值。

| 固定族签名 | 类型选择与行为 | unsafe |
| --- | --- | --- |
| `addr_of(place:T)->*kernel const T`; `addr_of_mut(place:T)->*kernel T` | 已知位置 T；后者须可写；不读目标或先构造引用；局部/本地不可变静态的普通位置取址安全，原始位置/static mut/extern static 取址 unsafe | 按来源 |
| `addr(p:P)->usize` | 任意原始指针，保留地址位 | 否 |
| `ptr::<T,space>(a:usize)->*space T` | 专用语法：显式具体 T、kernel/user/phys/mmio 标记；无默认空间；零得到空指针 | 是 |
| `ptr_const(p:*space T)->*space const T` | 同空间同目标只读弱化；除此之外无隐式指针转换 | 否 |
| `ptr_add(p:P,n:usize)->P`; `ptr_add_bytes(p:P,n:usize)->P` | 已知 P；保留空间/const/T；分别加 n*sizeof(T) / n；乘加溢出 panic；可越界计算，不授予访问权 | 是 |
| `read(p:*kernel const T)->T`; `write(p:*kernel T,v:T)->()` | 与普通 raw 读/写相同；read 的可变指针须显式 ptr_const | 是 |
| `copy_bytes(dst:*kernel u8,src:*kernel const u8,n:usize)->()` | 全范围已映射/存活/有权限；按调用前字节快照拷贝，允许重叠；保持未初始化事实；n=0 不访问字节 | 是 |
| `fill_bytes(dst:*kernel u8,v:u8,n:usize)->()` | 同样范围/权限；初始化每字节；不保证目标 T 合法；n=0 不访问字节 | 是 |
| `slice_from_raw_parts(p:*kernel const T,n:usize)->[]const T`; `slice_from_raw_parts_mut(p:*kernel T,n:usize)->[]T` | T 取 p 的目标；满足第 2 节切片合同 | 是 |
| `sizeof(T)->usize`; `alignof(T)->usize`; `offsetof(S,field)->usize` | 显式具体类型，field 为具名结构字段；常量依赖及静态 initializer 白名单由 L 闭合 | 否 |
| `volatile_load(p:*kernel const I)->I`; `volatile_store(p:*kernel I,v:I)->()` | I 为现存整数类型；自然对齐的一次同宽机器读/写，不可合并/删除/拆分；满足普通 RAM 合同 | 是 |
| `mmio_load(p:*mmio const I)->I`; `mmio_store(p:*mmio I,v:I)->()` | I 同上；一次同宽访问；满足设备方向/宽度/状态/权限合同，设备不套普通 RAM 初始化模型 | 是 |
| `atomic_load(p:*kernel const U)->U`; `atomic_store(p:*kernel U,v:U)->()` | U 只能 u32 或 u64；所有操作都是 SC，无 order 参数 | 是 |
| `atomic_exchange(p:*kernel U,v:U)->U`; `atomic_compare_exchange(p:*kernel U,expected:U,next:U)->U` | 返回旧/观察值；强 CAS，仅观察值==expected 时写 next，无伪失败 | 是 |
| `atomic_fetch_add/sub/and/or(p:*kernel U,v:U)->U` | 四个有限名字；返回修改前值；add/sub 按 U 位宽包装 | 是 |

没有独立指针强转：用 `ptr::<Dst,space>(addr(p))`，需要 const 结果再用 ptr_const。改变空间、目标或恢复可写拼写不会改变物理地址、建立映射、解除只读页或重新获得寿命/资源责任。user 数值还须平台用户拷贝；phys 数值还须真实映射；仅同值 kernel 重标记不能充当映射函数。
没有未对齐类型化读写内建和 packed；字节协议用 u8 读写与移位组装具体整数。普通内存复制不对 MMIO 或活动原子槽合法；大块移动由 copy_bytes 表达，不依赖不存在的库/宿主 C。

## 4. 原子、同步与架构族

原子槽就是独立自然对齐的 u32(4/4) 或 u64(8/8) RAM 区，无隐藏布局。先在未发布、独占阶段用普通写初始化；发布后到全部用户静止并重新获得独占权之前，同一槽只允许同宽原子操作，禁止普通/volatile/MMIO 访问及包含该槽的普通聚合复制。指针可复制不允许复制活动原子存储。
不同 hart、可重入的 IRQ/trap 都是执行上下文；冲突普通访问（至少一个写）没有 happens-before 或平台互斥时违反执行合同。静态对象不自动提供互斥，指针 const 不证明目标无并发写；共享可变静态与普通指针别名均受此合同约束。
源模型只保留旧 reference 22 的 SC 子集：每槽修改序包括初始化；全部原子操作参与一致全序 S，保持源序与各槽修改序；读看到 S 中此前最近修改/初始化；RMW 读紧邻前驱，失败 CAS 只读。SC 读有 acquire，写有 release，匹配观察建立 sw，hb 是源序与 sw 的传递闭包；禁止循环凭空制造值。具体沿用旧固定 N4861 基线的 SC/coherence 要求，不引入 C++ 生命周期/类型别名规则。
后端沿旧 psABI 原子映射或等价映射实现真正 SC；允许 AMO/LR-SC 重试，强 CAS 内部重试保留集丢失；无分配、睡眠锁或 wait-free 承诺，IRQ/trap 可用。普通 volatile/设备 fence 不建立原子 sw。
删除 u8/u16 原子：每个逻辑窄值用其独占 u32 槽，取值后显式掩码；不能把旧紧凑布局中的相邻字段当 32 位槽。继续不提供有符号原子：用等宽无符号位表示及显式转换。删除独立 usize 原子接口：用 u64 槽并显式转换，RV64上位宽相同。删除指针原子：用 u64 地址槽、addr/ptr 重建，空值用 0；目标对象发布/回收另需协议。不能原子更新聚合或混合宽度；用 u32 锁保护普通聚合。无独立 atomic_fence，须通过真实原子发布/获取协议同步。

以下 arch 名字保留，不能取函数指针，CSR number 是编译期已知 usize。每次架构操作保持源序、有副作用、作为编译器普通内存屏障；仅标有硬件 fence 的接口提供相应机器顺序。

| 固定签名 | 机械行为与检查 | unsafe |
| --- | --- | --- |
| `arch::riscv::cpu_relax()->()` | nop，不调度/睡眠 | 否 |
| `arch::riscv::wfi()->()` | WFI，可提前返回；等待/可能阻塞，禁止 trap 链调用 | 是 |
| `arch::riscv::csr_read(number:usize)->usize` | 读 CSR | 是 |
| `arch::riscv::csr_write(number:usize,v:usize)->()` | 写 CSR | 是 |
| `arch::riscv::csr_clear_bits(number:usize,mask:usize)->usize`; `csr_set_bits(number,mask)->usize` | 各一次 CSR 原子读改写，返回旧 CSR；第二个完整路径也在 arch::riscv 下 | 是 |
| `arch::device_read_fence()->()`; `arch::device_write_fence()->()` | 两者均 fence iorw,iorw；不建立普通原子 sw | 否 |
| `arch::riscv::fence_i()->()` | 本 hart FENCE.I，其他 hart 须另同步 | 是 |
| `arch::riscv::sfence_vma_all()->()` | 本 hart sfence.vma x0,x0，远程失效须显式平台协议 | 是 |

CSR 白名单沿旧 R7：0x100 sstatus、0x104 sie、0x105 stvec、0x140 sscratch、0x141 sepc、0x142 scause、0x143 stval、0x144 sip、0x180 satp。其余编译失败；写/RMW 仍须维持保留位、权限、WARL、映射和活动帧合同。
汇编保留唯一专用表达式 `arch::asm(STRING,in("a0") value,out("a1") place,clobber("t0")) -> ()`，整式 unsafe。输入仅整数/bool/原始指针；输出仅这些类型的可写位置，可尚未初始化；具体类型来自已声明位置，无类型反推。先按出现顺序求值保存全部输入，再按出现顺序保存全部输出地址，执行，最后按输出顺序写回。
沿旧 R8 的 a0-a7/t0-t6 集合、同寄存器最多一输入一输出、输入/输出扩展及 bool 必为0/1、局部数字标签、无 NUL 模板、无函数调用/外部符号/数据指令/非局部控制流/改 sp/ret/SRET、clobber 完整合同。汇编器与 unsafe 作者负责指令合同；Core 不解码证明机器代码。无纯汇编/nomem/noreturn/自动寄存器约束。一律副作用+编译器内存屏障，默认不插硬件 fence；允许模板修改内存但仍遵守原子/设备/存活协议。
删除旧源 `effects` 语法不解除运行限制；wfi、等待型汇编、未知外部服务是否可在 trap 调用由作者/平台承担，Extend 可只读检查显式事实。trap 禁止等待/睡眠/调度/主动启用 SIE，即使汇编可表达相应指令也不合法。

## 5. 布局、静态与 ABI

整数二补码、大小/对齐为位宽/8；usize/isize=8/8；bool=1/1且仅0/1合法；原始/函数指针=8/8，函数指针不得零（空槽用固定枚举或显式 stub）。unit/空结构=0/1。普通结构按声明顺序逐字段向上对齐，最终 sizeof 对齐到最大字段对齐，无重排/niche。
数组 `[N]T` 连续，步长 sizeof(T)，align=alignof(T)；N=0 仍保留元素对齐。切片16/8，地址偏移0、usize长度偏移8。零大小对象地址可相同，不从地址相等推导对象身份。
枚举 u32 标签按声明序从0编号；变体载荷按结构布局，载荷起点对齐到最大载荷对齐，联合大小取最大；整体 align=max(4,载荷align)，sizeof 尾部向上对齐。非法标签不能读；无载荷/非活动载荷不要求初始化；变体数超过2^32拒绝。若 L 保留 Option/Result，使用同一固定枚举布局，不利用空指针 niche。
`@align(N)` 支持1..4096的2次幂；类型属性提高类型对齐并尾部取整，对象属性只提高该对象地址对齐。布局加/乘/向上取整溢出 usize 编译失败。无 packed。
`@repr(C)` 只允许具名结构（整数/bool/kernel数据指针/C函数指针、递归此类非空结构与正长度数组字段），LP64自然字段布局；不开放 union/bitfield/flexible-array/空C结构，不能组合 @align。C可按此布局通过指针访问；通过其他对象指针并不授权 C 读取该对象表示。
`static` 只读，`static mut` 可写且读/写/取址 unsafe；初始化器常量与依赖由 L 定义，无运行时构造。只读常量/静态 .rodata，可写初始化数据 .data，可零填充者 .bss；删除内部可变包装器，不允许通过指针写本地只读对象。
`@section` 的非空无NUL节名、`@export` 的符号及 `__biomiya_` 保留前缀沿旧R1；权限不可由节名推断。链接脚本负责映射、对齐、KEEP/排序和边界；未满足构建失败。外部 static 不分配存储；其声明须具体合法布局，实际访问由平台保证。
链接地址符号可声明 `extern abi(C) static BOUND:u8`，仅 `addr_of(BOUND)` 不加载字节，也不要求存在可读对象大小；读取仍需真实存储合同。外部符号/本地静态取址按链接重定位处理，取址白名单由 L 确定。
普通 ABI 沿旧R3机械降低：标量按LP64；零大小参数省略机器参数但仍求值；非零聚合按值快照地址，聚合结果为首个隐藏结果槽指针、机器void返回；never无返回边。快照必须在每个实参求值时建立，不能等后续副作用后再复制；callee 参数是独立逻辑位置，切片只复制描述符。结果槽不与仍在读的快照重叠。
C 源签名仅允许整数、bool、kernel数据指针及具有同样 C 签名的 C 函数指针参数/结果；unit仅结果表示void。无聚合/枚举/切片/其他空间指针/never按值，无 variadic。C函数指针与普通函数指针不互转；C调用（含间接）unsafe，导出的C函数由声明者维护外部执行合同。
LP64 标量扩展、超过8参数后的栈传参、保存寄存器及对齐必须完整实现；不会因删除聚合C签名缩成“只支持八参”。聚合通过显式 `*kernel [const] S` 传入/传出，外部函数按协议返回标量状态码，不能依赖未定义 shim。

## 6. Panic、入口与陷阱

平台 `_start` 完成映像/.bss、16字节栈、gp/tp、映射后，可调用 `@export("kernel_main") extern abi(C) fn kernel_main()->()`。无隐式main。
panic 沿旧 `__biomiya_panic(message_address:*kernel const u8,message_length:usize)->!` 的物理LP64两参数ABI；平台记录/停机/重启，永不返回，不展开、不执行用户defer、不调用睡眠服务。后端复制/原子等辅助符号须列出且由最终构建提供，也不得引入睡眠、分配、构造函数或未说明系统调用。
`arch::TrapFrame` 公共 C布局不变：x:[32]usize、sepc、sstatus、scause、stval，偏移0/256/264/272/280，sizeof288、align8；平台可额外align16。x0=0，x2是被中断sp；后两字段是故障快照，返回桩不以其写硬件。
源特殊签名改为 `@trap_entry fn e(frame:*kernel arch::TrapFrame)->!`、`@trap fn h(frame:*kernel arch::TrapFrame)->()`、`@irq fn h(frame:*kernel arch::TrapFrame)->()`；无语言引用。三者不能普通调用/取函数指针；物理入口仍a0帧地址及普通保存规则。
`arch::enter_trap_handler(frame:*kernel arch::TrapFrame,named_handler)->!` / `arch::enter_irq_handler(...) ->!` 是 unsafe 尾转移内建，仅 trap_entry 尾部；具名第二操作数必须具有相应属性和同帧签名，不是可保存的源函数指针。帧须当前活动帧，不可局部仿造。先保存帧地址，然后执行该退出路径的defer/结束局部槽，再进入旧 `__biomiya_dispatch(frame_address,handler_address,kind)->!`（kind0/1）物理 ABI；handler 正常返回先执行自己的defer，再平台SRET。
硬件入口桩负责保存整数状态、内核栈、gp/tp、每hart帧/嵌套；处理期间SIE关闭。故障嵌套须保存/恢复活动帧或可靠panic，不能覆盖未处理帧。返回维护合法sepc/sstatus/x1-x31、映射/栈/权限并释放活动记录；帧指针在该次返回后失效。指针复制不证明当前帧身份或延长其寿命。
严格保留旧R6：trap/IRQ链不得调度、睡眠或主动开启中断；包括普通函数、FFI和汇编间接调用链。延后到普通检查点调度改变时延，不是时钟抢占；实际抢占/可睡眠系统调用须另命名配置并定义帧/栈切换，本候选不提供它。

## 7. 代表操作（语义伪代码，待 S 与 L 统一语法；没有编译证据）

页表条目：定义 `@align(4096) struct PageTable { entries:[512]u64 }`；平台page_alloc取得4096对齐/大小的存活RAM，失败返回空指针；fill_bytes全页为0，再通过addr+ptr重建PageTable指针。0表示平台Sv39无效PTE，未发布阶段初始化完成。子页表先初始化并保持存活，然后用atomic_store写发布的父表具体entry：`atomic_store(addr_of_mut((*table).entries[index]),encoded_pte)`。index按固定数组检查；对有效位/物理页号/权限及硬件A/D更新须遵守平台Sv39协议（并发更新用CAS保存需保留位）。修改后本hart sfence_vma_all，其他hart通过显式失效确认后方可回收旧页；纯指针重标记无此保证。

IRQ-save锁的完整顺序（所有 raw/CSR/atomic 操作在 unsafe 中，锁槽独占u32且预先初始化为0）：
1. `saved:usize = arch::riscv::csr_clear_bits(0x100,2)` 一次读取旧sstatus并清SIE；不是read后write的窗口。
2. 循环 `atomic_compare_exchange(lock,0,1)`，观察0才获得锁；否则cpu_relax；持锁内访问普通受保护数据。
3. 正常退出用显式 unlock 或 defer：先 `atomic_store(lock,0)`，再仅当saved&2非零执行 `csr_set_bits(0x100,2)`。清理按此单个函数顺序展开，不把开中断放在释放前。
4. 嵌套irq-save按LIFO恢复；持有区禁止主动启用SIE、睡眠或调度。trap内saved的SIE为0，所以不会主动开中断。panic不解锁/defer，平台应停机而非继续执行。跨上下文锁交接必须显式协议，不能让词法defer提前释放。

MMIO：`reg:*mmio u32=ptr::<u32,mmio>(mapped_device_address)`；准备DMA描述符普通RAM后device_write_fence，再`mmio_store(reg,command)`；如设备要求命令后顺序再执行device_write_fence；读`mmio_load(ptr_const(status_reg))`并按设备协议调用device_read_fence后读已完成的DMA RAM。宽度/方向及顺序来自具体设备协议；设备fence不代表DMA已经完成，完成前不得释放缓冲区。映射由平台完成，物理数值不冒充设备虚拟地址。

C聚合：`@repr(C) struct Request { base:*kernel u8, size:usize }`；调用 `extern abi(C) fn submit(req:*kernel const Request,out_id:*kernel u64)->i32` 时传addr_of(req)/addr_of_mut(out_id)，返回0后才可读已写out_id；非0状态必须按此具名函数合同处理，不假定输出初始化。所有传参分别求值一次；Request不按值经C。

显式 ctx+ops：具体 `FileOps` 结构字段为 `unsafe fn(*kernel u8,[]u8)->isize` read、`unsafe fn(*kernel u8)->()` retain/release；`FileHandle` 保存ctx:*kernel u8与ops:*kernel const FileOps。读先复制ops.read指针，再显式调用(ctx,buffer)；具体驱动包装函数用addr+ptr恢复其具体State类型，检查合法配对/存活并实现实际操作。接口无需trait/用户泛型/方法；ctx和ops配对与最后引用销毁不是类型保证。

失败回滚与句柄转交的准确展开：分配FrameRange后登记带布尔armed状态的defer，失败出口若armed则release；初始化DMA描述符/映射表之前保持armed=true。插入成功后表接管释放责任，显式armed=false；插入失败保持true或显式release后置false。map.remove先在锁内取走具体记录，释放锁后release，不能在锁内运行可能再次取锁的销毁。FD duplicate/get调用retain；insert“消费责任”只在成功后生效；replace先保存旧句柄再安装新句柄，解锁后release旧句柄；close清空槽再release。表销毁显式逐个release。
所有这些“转交”只是执行协议：普通赋值/按值参数仍复制句柄，Core不把源置空或防止重复release；调用者必须显式清空/标记旧槽并停止再使用旧责任。复制独立句柄前须retain，观察性副本不得release；泄漏/双重释放及并发回收风险由库合同/可选证明承担，不能假装旧移动所有权检查仍在。

## 8. 可观察验收边界

未来实现须检查：原始槽字段初始化后合法聚合读取/非法未初始化读取的合同、各布局与TrapFrame偏移、快照早于后续实参副作用、C九参和聚合指针往返、SC CAS强失败语义/IRQ-save恢复顺序、MMIO单次正确宽度、PTE发布/远程确认前不回收、defer回滚/转交后不重复释放、panic不展开、trap内禁调度合同。负例应拒绝非kernel普通解引用、const写、C聚合按值、u8原子、动态/非白名单CSR、普通调用特殊函数。
本节点只给闭合候选及内存/平台执行合同；没有实现这些检查或模型、没有测试内核移植、没有编译器净行数测量。L 负责常量/静态依赖判定与语言退出语义；S 须核对两个文件的一致性后才能成为最终设计。
