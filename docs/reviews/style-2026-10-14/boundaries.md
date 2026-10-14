# B：Q 的所有权、错误和机器边界审计

2026-10-14；作者 `/root/biomiya_style_boundaries`；基线 `2c7061cbed13959697369879e5717a8ad0c9fb21`。标尺为 `suba/CODE_STYLE.md` 0.1。本文是作者候选，独立验收由 root 记录。

本轮重点阅读当前 Q 的 `model/input/image/ir/rv64/s0/blocking/main.vt` 的存储声明、公共入口、生命周期、失败和发射路径，以及 `next/q/runtime/{linux,bytes}.vt`、`linux.s0`。辅助对照 `ownership.md`、`tests/owner.vt`、`tests/input-tests.md` 和写失败 shim。路径若未写前缀，均相对 `core-s/next/q/`。未逐条重新证明所有 RV64 指令编码、IR 类型规则，也未重审冻结 B/P/Rust；尤其 `core-s/runtime/linux.vt` 是历史版本，不冒充当前 Q runtime。覆盖清单和源文件哈希见 `boundaries-freeze.json`。

结论：先修复一个已复现的路径语义缺陷，再收紧错误与 unsafe 合同；现有实际 owner/epoch/seal 架构应保留。不能根据“大量 unsafe / 全局 static”推断重构无效，也不能把单次用户态编译器的整文件读写、递归或退出诊断按内核 IRQ 规则拒绝。

## B1［P2，功能缺陷］remove 不拒绝内嵌 NUL，删除了调用者给定切片的前缀路径

- 位置：`runtime/linux.vt:65–75`，尤其第 70 行；对照 `open` 第 35–42 行。
- 规则：§5 外部字节显式解码，§6 边界验证，§8 ABI 转换意图和前置条件。`open` 已拒绝 NUL，`remove` 只检查长度、复制，然后直接交给以 NUL 结束的 Linux 路径 ABI。
- 触发：给公开 `remove` 一个内容为 `victim\0x` 的合法存活字节切片。其长度为 8，不能在 Unix 文件名中表示；当前实现把它变成 `victim` 并成功删除。这里不涉及悬空指针或非法内存。
- 真实探针：冻结 E3 编译当前 runtime，在全新临时目录创建唯一哨兵 `victim`。`open_read` 同样的切片返回 11 且文件仍在；`remove` 返回 0，哨兵消失。见 `boundaries-probe.py` 和 `boundaries-probe.json`。编译器 SHA 为 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。
- 影响范围：公开 runtime 的路径合同缺陷。当前编译器 `input::load` 经 `bytes::path` 排除 NUL，因此本轮没有证明合法 manifest 能借此删除其他文件，不应升级为编译器任意文件删除漏洞。
- 建议：把路径复制和 NUL 检查收敛到一个具体动作，由 open/remove 共用；至少先补 remove 的 NUL 拒绝。补“开和删都拒绝内嵌 NUL、原文件不变”测试；不需要通用 Path 框架。

## B2［P2，失败诊断/残留物缺口］保存失败把多个阶段压成 bool，清理失败没有可见结果

- 位置：`input.vt:353–387`；`runtime/linux.vt:48–50,60–75,95–97`。
- 规则：§6 失败必须传播或明确处置，保留 operation/phase/实际错误；§7 发布前失败撤回，关闭/flush 结果显式处理。
- 当前正面：`create_new` 避免覆盖既有输出；写完后显式检查 close；失败会尝试 remove，已有短写和单次写失败回归。因此“所有失败都泄漏 fd/残留输出”是错误结论。
- 实际缺口：写、chmod、close 错误统一变成 `bad`；删失败返回值在第 386 行被忽略，无注释说明后果。即使磁盘留下一个部分输出，用户只得到 `E_IO file=0 byte=0`，不知道操作、路径、阶段、是否清理成功。runtime 又把真实 errno 合并为 12/13/14/16；主错误本身也已丢失。
- 有界注入：复制真实 runtime/S0 到临时目录，第一轮 32 字节输出只写 7 字节，下一轮写返回 -5，unlink 返回 -13，其他 syscall 实际执行。结果 exit 1，输出 `E_IO file=0 byte=0`，`partial.elf` 留下 7 字节 `0123456` 且 mode 0755。见 `boundaries-cleanup-probe.py/json`。这是双故障注入，不能宣传为正常保存必现。
- 建议：用小型具体 `SaveError { phase, code, cleanup_failed }` 或对应枚举保留主错误与清理结果，在拥有输出路径的边界诊断。失败写之后无需再 chmod；只有完整输出才设可执行。unlink 被文件系统拒绝时不能保证回滚，应该明确报告残留路径和失败事实，而不是无限重试或添加不必要的事务系统。继续保留显式 close 的优点。

## B3［P3，所有权维护偏离］统计和终端诊断仍是公开可变状态；同一统计有重复维护者

- 位置：`model.vt:381–386`；实际外部写入 `ir.vt:316–317`、`rv64.vt:1217–1218`、`check.vt:2067–2071`、`parse.vt:576–577`；读取 `main.vt:180–182`。
- 规则：§2 可变 static 仅在所有者模块私有，§3 权威状态与更新点、接口表达动作。
- 影响：`max_ir` 同时由 IR owner 和 checker 更新；`max_ast` 同时由 parser 和 checker 更新。每条写入目前是单调 max，本轮未发现统计错误，但维护者不能从声明判断唯一更新责任，任何 import 者还可清零或伪造值。三个 error 字段也不必开放任意写入才能支持终端诊断。
- 建议：统计由其真正 owner 维护并提供只读 snapshot，或在 model 中用 `observe_ir_count` 等小动作集中更新；二选一，消除重复更新而非再包一层仍允许多方写。错误状态私有化，必要时提供只读诊断快照。不要因这几个标量把已私有化的源码池、语义表和映像全部改成 Context。
- 不扩大结论：新规约在实际修改范围逐步实施；这属于既有明确例外的治理债务，并不推翻已通过的 compiler release。check 自有三个公开统计由另一审计节点覆盖。

## B4［P2，K0 移植前置项；不是 Linux 版本功能 bug］平台与目标策略仍穿过多个语义模块

- 位置：宿主诊断 `model.vt:75,388–400,455–456`；输出 chmod `input.vt:5,368–375`；目标入口/终止 `rv64.vt:1294–1313`；固定目标地址 `image.vt:484–490`；清单只接受 linux-user `input.vt:265–272`。
- 规则：§3 依赖方向/模块职责，§8/§10 ABI 与平台常量的权威定义，§11 隐藏宿主依赖不可混进正式交付。
- 具体耦合：`model` 直接 Linux write/exit，`input` 直接 chmod；而 `rv64::start` 发出的被编译程序又直接用 Linux exit 93，并假定 x27 是 Linux 初始栈。前两项是编译器所运行环境，后者是输出目标，不能混在一个“换 runtime”开关里。
- 影响：编译器迁移到 suba 或输出 suba/kernel 程序时，只换 `runtime/linux.vt` 会漏掉现有 syscall 和启动 ABI。这是当前 Linux-only profile 的已知限定，不是当前 linux-user 的错误，也不要求为了审计立即支持新平台。
- 建议：K0 开始先把 host I/O/诊断终止收敛到明确平台模块；单独用固定具体 TargetProfile 选择入口/终止桥和映像布局，复用 RV64 指令编码。无需函数指针、插件接口或通用后端框架。验收分别覆盖 Linux 行为不退化、suba 输出不包含 Linux-only 桥。

## B5［P2，安全接口合同债务］公共裸切片 API 的 unsafe 承诺不一致，局部证明不足

- 位置：`runtime/bytes.vt:3–10` 的 safe `equal` 对照第 13–16 行 unsafe `view`、第 23 行 unsafe `path`；`input.vt:119–138` 的 safe `add_source`；`runtime/linux.vt:57–58,65–75` 的 safe 路径 API；第 79、91、126 行 unsafe I/O API；S0 `runtime/linux.s0:1–19`。
- 规则：§8 公共函数无法自行建立存活期/访问权时声明 unsafe 并写调用者责任；非显然区域有就近 SAFETY 证明；§10 机器入口交代寄存器和 ABI。
- 具体问题：当前语言的切片仅复制描述符，长度验证不能证明 RAM 仍存活。上述 safe 入口内部直接解引用调用者切片、复制到 owner，无法自行证明源存储有效。`add_source` **复制后**所有权很好，但复制过程中仍需输入可读。现有 unsafe I/O 入口也没有就近写清 buffer 的权限/有效期、等待期间必须保持有效及失败后的部分写语义。S0 只统一提“Linux C ABI/保留x27”，argc/argv 对初始栈、参数上界和存活期的依赖需要从实现反推。
- 限定：这是新规约下的接口合同缺口，不声称普通合法程序能不经任何 unsafe 制造坏指针，也不因为缺少 SAFETY 注释就推断已有内存破坏。当前 main 给 input 的 buffer 来自存活 static，已审的实际编译路径有合理有效期。
- 建议：对接收外部切片且无法建立有效期的公开动作明确 unsafe 和最小调用者责任；在来源本来由 owner 保证的路径用具体内部合同注明有效到哪一阶段。用小型范围/解码 helper 与动作边界缩小 unsafe，避免按每行机械拆块。runtime S0 每个入口补参数寄存器、返回/破坏寄存器、sp/x27 前提。无需给所有安全函数统一加 unsafe，更不需要逐 helper 镜像测试。

## 已满足且应保留的结构

- `input.vt:23–47,63–114,119–138`：输入真实复制进 owner，freeze 拒绝追加；header 只完成一次、完成后拒绝追加 import；外部读的是快照而不是可写表。`ownership.md` 正确描述了此边界。
- `image.vt:13–46,54–74,428–577`：符号/数据/reloc/fixup/输出字节都在 image；begin_compile/begin_text/finish 区分阶段；finish 返回 const 输出并封口。没有回到 public RawImage setter。大 finish 函数可在实际修改时按布局/重定位/ELF头三个动作整理，但长度本身不构成本轮功能缺陷。
- `ir.vt:174–212,309–339` 和 `rv64.vt:1119–1248`：函数 ID 世代与边界检查、封口、revision 绑定及单次发射票据都存在。`sealed` 与 `closed` 分别表达 IR 构建许可与基本块终结，并非应机械合并为“非法布尔组合”。
- `rv64.vt` 只看 typed IR/slot，未读取 source AST；frame_offsets 由后端私有持有。指令编码常数集中在 encoder，不能把每个 opcode 字面值都算“魔法数字缺陷”。
- `blocking.vt:7–29` 明确位是独立性质；MAY_BLOCK/UNKNOWN 与有界单调队列适合位集。其 bool 校验结果并不天然违规，不能为了规约一律改枚举。
- `runtime/linux.vt:79–142` 对 read/write 处理 EINTR、短读写和 read_file close；当前系统调用阻塞是用户态编译器的合法操作。本轮未套用内核 step/IRQ 的时延硬约束。

## 验证与改动边界

只新增本目录 `boundaries*` 审计文本、脚本与 JSON。两种真实 QEMU 探针共两次编译、三次运行，均采用冻结 E3 和当前生产模块的副本。临时目录随运行删除；只保留文本源码、注入 shim、输入哈希和结果，脚本可重现。生产代码没有修改，历史 release/验收证据没有覆写，未重跑完整编译器测试（本轮没有生产变更）。

建议次序：B1 小修和反例回归 → B2 保存失败合同 → B4 纳入既定 K0 → 在对应模块修改时处理 B3/B5。B4/B5 不能替代 K0 的真实目标验收，B3 也不值得单独启动全库重排版。
