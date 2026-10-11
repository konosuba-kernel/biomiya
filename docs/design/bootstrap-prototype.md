# B0/B1：真实源码到直接 RV64 ELF 原型

2026-10-10。本次交付是 bootstrap-v1 的 B0/B1；沿用 `core-s-pilot.md` 的既有语言子集，不冒充 R1a、完整汇编器、操作系统或自编译。现有单元模块没有命令入口和真实源码集成证据，须实际接通并修复集成暴露的错误。

## 架构与共享接口 H r2

```text
源文件 → frontend::parse → frontend::resolve → check::check
                                                ↓ Compilation
                                      backend::machine
                                                ↓ machine::Program
                               ┌────────────────┴───────────────┐
                          汇编调试输出                       encode::encode
                          Clang/LLD 对照                        ↓
                                                          image::executable
                                                               ↓
                                                         ELF → qemu-riscv64
```

`core-s/src/machine.rs` 是冻结的机器层数据合同。它保存类型明确的寄存器、操作码、立即数和符号；不能把现有汇编字符串交给新解析器再称为直接机器输出。前端 AST、检查后的 CFG 与机器操作分别有明确责任，后端不重新检查源语言。

后端节点提供 `backend::machine(&Compilation)->diagnostic::Result<machine::Program>`。保留 `backend::emit(&Compilation)->Result<String>` 供既有测试、参考工具链和人工检查；它渲染同一份 machine 输出，不维护第二份语义 lowering。添加 `backend::assembly(&machine::Program)->Result<String>` 供整程序参考输出；它允许尚未定义的外部符号，由参考链接器最终处理，不能解析文本生成 Program。

编码节点提供：

```text
encode::Encoded { bytes:Vec<u8>, symbols:BTreeMap<String,u64>, fixups:usize }
encode::encode(&machine::Program, base:u64)->diagnostic::Result<Encoded>
image::TEXT_BASE: u64 = 0x10000
image::executable(&machine::Program, entry:&str)->diagnostic::Result<Vec<u8>>
```

Label 值为最终地址。编码器在同一次构建中解析全部标签，拒绝重复／未定义名字、无效寄存器、立即数和偏移越界、基址未按 4 对齐、地址溢出，代码跨度最大 64 MiB。内部损坏输入用 I_INTERNAL，容量／范围超限用 I_LIMIT。Branch 固定 12 字节，Jump／Address 固定 8 字节；不做松弛。Li 按确定算法展开任意 u64 位模式，先确定大小再布置标签；测试须包含最高位、全一和高低立即数进位。

控制流展开固定使用 x31/t6 作为 scratch，不能破坏 t0..t5 中尚需使用的值；Address 用自己的 rd，禁止 rd=x0。此处修正 bootstrap-v1 早期示意中的 x5，保留现有后端大量使用 t0 的序列。Jump 的 rd 只允许 x0/x1，避免未定义的调用变体。其他寄存器编号必须小于 32。

ELF64 little-endian ET_EXEC、EM_RISCV=243、e_flags=0、入口处于 text；三个 PT_LOAD 依次 RX/R/RW，4 KiB 对齐，文件首个页放头部而不映射。text 在 0x10000；原型的 rodata/data 各用一字节占位，不声称支持源静态数据、ABS64 或 BSS 声明。无 section header、解释器或动态依赖。空机器程序／不存在的入口必须拒绝。初始化段的文件和地址边界、权限及 padding 用测试核对。

## 运行与命令行合同

库层 `pipeline::compile(FileId,&str)->Result<Compilation>` 串接 parse、resolve、check，每个错误原样保留，不做旧编译器 fallback。现有 Core S 的常量布局图与 R1a 规则不同，本轮不偷偷迁移语义。

最终 CLI：

```sh
core-s check input.vt
core-s build input.vt --entry run -o output.elf
core-s asm input.vt --entry run -o output.s
```

build/asm 的入口必须是无参数、非 unsafe、返回 u32 的函数；check 不要求入口。真实 Linux 适配由机器操作构造 `_start`，调用选定函数，把返回值交给 Linux exit(93)；panic 退出 101。0..255 是可观察退出状态；用例期望 101 时必须注明 panic，不能把全部非零当执行失败。运行时不使用 libc，也不承诺完整 Linux ABI 或 SOS ABI。正常入口可以返回 101，所以通用 CLI 不将进程退出 101 当成可证明的 panic；测试以受控用例判定路径。

build 不调用外部工具；asm 只输出参考汇编。失败显示错误码、文件和源位置，退出非零；先完整编译，后通过同目录临时文件加 rename 发布输出，失败不覆盖已有产物。构建产物确定，不含时间戳／宿主路径。输入和输出为同一路径（含已存在的硬链接／符号链接别名）时拒绝，避免覆盖源码。

## 节点与独立验收

| 节点 | 依赖 | 实施范围 | 验收重点 |
| --- | --- | --- | --- |
| H 架构与接口 | 无 | 本文、machine.rs、feature/module 声明 | 接口闭合，编码／运行可实现，验收不冒充自举 |
| E 编码与映像 | H | encode.rs、image.rs、tests/image.rs | 立即数／远跳／坏输入；参考工具编码核对；ELF 段 |
| B 后端机器输出 | H | backend.rs、tests/backend.rs | 不重解析汇编；原后端行为回归；scratch 和调用合同 |
| P 源码管线 | H | pipeline.rs、tests/pipeline.rs、必要 frontend/check 修复 | 源码快照／defer／Result／诊断；错误不得被吞 |
| I 集成与交付 | E、B、P | main.rs、tests/cli.rs、fixtures、README、统计与工作流 | 真源码执行；直接／参考路径对照；无外部工具 build；清洁构建 |

主代理负责 H/P/I；后端与编码分别由不同 agent 实施。独立 reviewer 不修改受验产物，审查 H/E/B/P/I，各节点根据内容摘要冻结候选，修复增加 revision，按实际影响重验。详细状态与身份保存在 `docs/workflows/bootstrap-prototype/`。

必要集成覆盖：整数／分支、超过八参数、聚合快照、返回快照与 defer、Result 错误传播、溢出／除零／边界 panic、语法／名字／类型错误、I_UNSUPPORTED、损坏输出路径、既有输出保护及确定性。不能以手工 IR 替代这些源程序证据；机器级边界允许独立构造 Program。

不增加语言特性、不实现 .s0 解析器、manifest、多文件、文件运行库、SOS 或自写编译器。记录新手写模块、直接复用源码、Unicode 表、测试和平台适配的物理行数，当前不是最终规模承诺。最终说明须分别列已完成步骤与尚未实现能力。
