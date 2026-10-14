# P5 系统切片与目标平台

这些文件实现 P5 系统切片。Q 首个自举候选 E3 已编译并运行 `system.vt` 与 `file_rollback.vt`。独立验收发现初版 monitor 的非法 PTE 高位绕过，修订 r2 修复后通过；证据为 `build/snext-p5-r2/verification.json`，判定见 [P5 审查](../../docs/history/s-next/reviews/p5.md)。架构返工后，最终交付使用 `build/snext-p4-r2/e3.elf` 重跑并记录到 `build/snext-p5-r3/verification.json`；此命令和预定路径不表示验收已经完成，实际判定见 [DAG](../../docs/history/s-next/delivery.md)。

## 可执行边界

- `system.vt` 是一个完整程序：16 项位图超页分配器、第二次申请失败时回滚第一次资源、具体变体错误、Sv39 页表编码、双向地址别名验证、4,096 次双 hart 原子交换、UART 输出。
- 页单位明确为 **2 MiB**，使用 Sv39 level-1 叶子，不伪称 4 KiB 页或完整内核物理内存管理器。池为 `[0x85000000, 0x87000000)`；返回 PageId，不返回假装拥有内存生命周期的语言引用。
- `file_rollback.vt` 在 RISC-V Linux user 模式打开真实 `/dev/null` 为只写，然后 read 得到 EBADF。调用式 defer 关闭 fd；第二次 close 得到 EBADF 证明资源已释放。清理函数的错误策略是记录失败并继续，样本显式检查该记录。
- `probe.vt` 是旧语法环境探针，单独记账，不是新语言应用。它只验证平台能运行现有直接 ELF、页表实际生效、双 hart 协议和设备访问。

## 运行命令

从仓库根目录 执行，输出目录必须新建，已有证据不会覆盖：

```sh
# 已通过的提前探针，使用冻结 B：
/usr/bin/qemu-riscv64 -s 1048576 build/b31-refactored.elf build experiments/system/probe.build
python3 experiments/system/run_system.py build/p5-probe.elf --out build/p5-probe-new

# 使用明确登记的 Q/E3 候选：
python3 experiments/system/check_system.py --compiler build/snext-p4-r2/e3.elf --out build/snext-p5-r3
```

第二条验收命令分别编译新语言系统与文件切片，在空 PATH、1 MiB guest 栈下运行目标编译器；随后运行 Linux 文件切片与 `qemu-system-riscv64` 双 hart 切片。报告记录编译器、样本、适配器、宿主工具、命令、输出和哈希。它只证明该候选的系统行为；正式 P5 还必须由调用者把候选哈希与 P4 完整固定点证据绑定。

冻结 B 的提前记录 `build/p5-probe-r3/verification.json` 仍保留 `status: preliminary`。Q/E3 的 r2 证据重新运行完整新语法程序，UART 为 `P5 OK\n`，退出 0，并覆盖 10 项平台负例：保留槽位、非法 flags、未对齐 PPN、未映射读、PTE 保留位及物理范围越界均以 250 退出，显式失败以 7 退出。每个报告绑定实际使用的编译器和平台源哈希，不能替换哈希后继承旧结论。

## 机器合同

`machine.s0` 只有四个具名 C ABI 入口，执行固定 ecall 号。实际机器操作全部在显式计账的 `monitor.S`。编译器把 extern 看作有未知内存副作用的调用；这个边界不是可以任意重排或缓存的纯函数。

| 入口 | 合同 |
| --- | --- |
| `p5_uart(c)` | 单 hart 调用；轮询 16550 LSR 的 bit 5，每次 `lbu` 宽度 8 位；THR 每次 `sb` 宽度 8 位，只发送低 8 位；发送前后 `fence iorw, iorw`。 |
| `p5_exchange(v)` | 单请求方 hart 0；请求 flag 为对齐 u32，payload/result 为对齐 64 位；hart 0 release 发布、hart 1 acquire 消费，hart 1 release 发布结果、hart 0 acquire 读取。返回 `v+17` 的机器位模式；样本输入范围不会溢出。它是固定同步邮箱协议，不是通用无锁队列或语言级线程库。 |
| `p5_install(index,pte)` | 改变当前 Sv39 level-1 页表项；只允许 `[32,128)`，flags 必须 V/R/W/A/D，无 U/X，bits 63:54 必须为零，PPN 必须 2 MiB 对齐。直接在 PPN 域比较专用 RAM 范围 `[0x85000,0x87800)`，不先左移截断；写页表后 `sfence.vma`。 |
| `p5_inspect(index)` | 读取实际安装的页表条目；样本还通过两种地址访问同一物理页，不能只靠读回条目判断翻译正确。 |

原子使用 RV64A 的 `amoswap.w.aq`、`amoswap.w.rl`、`amoadd.w.aq`，两条真实 QEMU hart 同时执行。`aq`/`rl` 的内存域顺序与设备域 fence 分开定义；可参考 [RISC-V A 扩展规范](https://docs.riscv.org/reference/isa/unpriv/a-st-ext.html)。QEMU 的 `virt` 提供 NS16550 UART，见 [QEMU virt 平台说明](https://www.qemu.org/docs/master/system/riscv/virt.html)。这是 ISA/设备仿真目标执行，不是实体硅片验证，也不是把 UART 寄存器换成普通 RAM 的假 MMIO。

## 加载与宿主依赖

目前 ELF 固定低虚址、入口以 Linux exit ecall 返回。`run_system.py` 验证 ELF64/RISC-V/段界限后，逐个 PT_LOAD 放到物理 RAM，并显式补零 BSS；**不改任何 guest 指令、重定位或 ELF 字节**。Sv39 把 guest 低 64 MiB 映到 `0x81000000` 起始 RAM；另提供高 RAM 直接映射及 `[0x80500000,0x80600000)` 的 1 MiB 栈预留。程序仍为 S-mode，只有 exit 与四个机器接口通过 monitor 进入 M-mode。没有 Linux 文件系统服务模拟器，文件切片单独在真实 Linux syscall 环境执行。

monitor 是测试平台，不是 SOS 的替换内核；当前既没有 ELF 加载的目标自举实现，也没有内存保护隔离、调度、中断、文件系统或 SOS 内编译。全 RAM 身份映射是受信实验合同，程序不能被当作不可信用户进程运行。

宿主 clang/ld.lld 仅构建 `monitor.S`，QEMU 仅加载样本 ELF；不会调用宿主编译器代编译 VT。正式证据必须同时记账这两个外部工具及 128 MiB QEMU RAM；它们不算 S-next 语言自身实现，也不隐藏为零成本。现有 S0 不支持 fence/AMO/CSR，所以这些特权操作在这一有限平台接口中。若以后将其变为直接 S0 指令，需要另测编码、ABI 和顺序，不能自动继承当前接口的验收。

## 后续正式验收检查点

1. P4 接受的同一 E3 编译两个 S-next 样本；记录新候选 hash 与独立行为结果。
2. 确认 make、zero、穷尽具体 match 的语法与正式参考一致，不能为样本临时恢复旧 Result。
3. `check_system.py` 会通过 `check_monitor.py` 重编译并重跑 10 项平台负例；旧 B 产物的负例通过不能替代 Q 候选测试。
4. 统计编译器、库/平台、应用、测试与工具。所有汇编及加载器都纳入交付成本。

当前切片没有要求新增整数宽度、间接调用、用户泛型或更强类型推理。代价是硬件接口固定、机器环境受限、部分实际设备操作位于已公开的平台汇编中。
