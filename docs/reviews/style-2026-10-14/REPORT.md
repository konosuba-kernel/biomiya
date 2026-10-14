# biomiya 代码与风格规约审计

2026-10-14；基线 `2c7061cbed13959697369879e5717a8ad0c9fb21`。
标尺：[suba/CODE_STYLE.md 0.1](../../../../suba/CODE_STYLE.md)。本轮只审计，未修改生产代码、冻结种子或旧验收证据。

**结论：现有编译器的主体分层成立，但尚不能作为新规约下的示范实现。优先工作是修复错误路径和收紧接口合同，不是再做一次全面重写。** 本轮复现了四类行为问题，另外识别出五项结构或迁移债务；没有据此推翻既有自举验收，也没有证明新的生成错码。

## 1. 已复现的问题

优先级 P2 表示应安排修复；P3 表示随相关修改整理。这里没有使用行数直接确定优先级。

### F1 · P2：验证工具可能把错误结果记录为通过

位置：[check_q.py:92](../../../core-s/tools/check_q.py#L92)、108、130；规约 §6、§11。

`check_q.py` 把运行结果、拒绝诊断、输入哈希等关键判定写成 Python `assert`，却没有阻止优化模式。`python -O` / `PYTHONOPTIMIZE` 会移除断言，随后仍执行 `passed = True`。

隔离探针使用**逐字复制的原工具、真实 E3 和 QEMU**，只把测试语料换成一个实际返回 0、故意期望 7 的程序：普通 Python 报失败并 exit 1；`python -O` 报 `All 1 Q language cases passed.` 并 exit 0。见 [探针](probe_asserts.py)和[结果](asserts-evidence.json)。这不是模拟编译器，也不是发布测试重跑。

同类断言存在于 `check_q_interfaces.py:56–68`、`check_q_input.py:96–98`、`check_q_copy.py:131–165` 等工具；这些是静态确认的同类位置，未逐个运行优化模式反例。已有 `selfhost_next.py:180` 用显式 `require(__debug__, ...)` 拒绝优化模式，可以沿用，或改成不可优化掉的明确检查。修复后的关键反例必须仍拒绝；不需要先抽出通用测试框架。

**限定：**正常 `python3` 运行的既有证据不因这个发现自动失效。问题是工具在另一种可启用配置下产生假阳性。

### F2 · P2：删除路径时接受 NUL，实际删除了前缀路径

位置：[runtime/linux.vt:65](../../../core-s/next/q/runtime/linux.vt#L65)，对照35–42；规约 §6、§8。

`open` 拒绝路径里的 NUL，`remove` 只检查长度。对内容为 `victim\0x` 的合法存活切片，前者返回11且文件保留，后者返回0并删除临时目录中的 `victim`。此反例不依赖悬空指针。

应共用具体的路径复制/验证动作，拒绝 NUL 后再进入 Linux ABI；增加开/删一致拒绝且原文件不变的回归。当前 manifest 的 `bytes::path` 已拒绝 NUL，所以本轮发现的是公开 runtime 的合同缺陷，**不是已证明的 manifest 任意文件删除漏洞**。证据：[B1](boundaries.md)、[root独立复跑](boundaries-root-remove.json)。

### F3 · P2：保存失败丢失清理结果，部分文件可能静默残留

位置：[input.vt:353](../../../core-s/next/q/input.vt#L353)，重点368–387；规约 §6、§7。

写、chmod、close 的错误被合并成 `bad`；remove 返回值又被忽略。注入“先短写7字节，再写失败，同时unlink失败”，实际留下7字节、0755权限的 `partial.elf`，但仅输出 `E_IO file=0 byte=0`。部分文件带可执行权限不表示它是可运行的有效 ELF。

应保留保存阶段、主错误与清理失败信息，报告可能残留的路径；写成功后才设置可执行权限。文件系统拒绝 unlink 时无法保证删除，应诚实报告，不能承诺无限重试或靠增加事务系统解决。当前 `create_new` 保护已有文件、显式close和普通失败清理都值得保留。

这是**双故障注入**，不是正常保存必现。见 [B2及探针方法](boundaries.md)、[root独立复跑](boundaries-root-cleanup.json)。

### F4 · P2：字段错误的诊断位置被前一个字段值覆盖

位置：[check.vt:1036](../../../core-s/next/q/check.vt#L1036)、[init.vt:180](../../../core-s/next/q/init.vt#L180)、`types.vt:28–37`；规约 §3、§6。

构造 `make Pair { a:11, missing:22 }` 时，递归检查 `11` 更新了全局诊断span，随后检查 `missing` 没恢复当前字段位置。因此诊断落在合法值 `11`。局部构造、静态构造，以及未知/重复字段四案例，check/build八次调用均复现；所有案例仍被正确拒绝，没有输出 ELF。

先在字段校验边界传入/恢复当前span，漏字段错误恢复整体构造span；逐步减少“locate→任意递归→fail”的隐藏时序依赖。无需把整个编译器改成统一错误框架。验收应包含当前字段位置、跨模块私有字段、漏字段及IR来源位置。见 [A1](frontend.md)、[原始输入](frontend-probes.json)、[root独立复跑](frontend-root-verification.json)。

## 2. 结构与合同债务

| 项目 | 证据和影响 | 建议与时机 |
|---|---|---|
| M1 · P2：裸切片接口责任不完整 | `lex.vt:274`、`parse.vt:103–136`、`input.vt:119`、`runtime/bytes.vt:3`。安全入口内部解引用外部切片，但长度不证明存活期/访问权；一些unsafe入口也没有调用合同。对应 §8 | 结合输入/平台边界修改补unsafe签名、来源与有效期证明；owner制造的有效视图可保留安全接口。未发现真实悬空调用，不能把缺注释等同内存漏洞。详见A2/B5 |
| M2 · P2：InitData用魔数和重载字段表达互斥载荷 | `init.vt:6–23,52–70,255–274,461–516`。target既是符号又是次数；bits既是值又是长度/步长。对应 §3、§5 | 改为具体InitValue枚举和语义构造操作；保留建树/序列化分层，量测arena与复制成本。当前未证明传错参数。详见A3 |
| M3 · P2：宿主环境与输出目标尚未分离 | `model.vt:388–400,455–456`直接Linux诊断/退出；`input.vt:368`直接chmod；`rv64.vt:1294`又给输出程序生成Linux初始栈/退出桥。对应 §3、§10 | 纳入既定K0：宿主I/O/终止和具体目标profile分别处理，保留RV64编码器。只换runtime不足。**这是Linux-only版的明确边界，不是现版本功能缺陷**。详见B4 |
| M4 · P3：9个公开可变标量，统计存在重复维护者 | `model.vt:381–386`、`check.vt:74–76`；max_ir由ir和check更新，max_ast由parse和check更新。对应 §2、§3 | 由真实owner维护并给只读snapshot，或集中observe动作；去掉重复写入。当前写法单调max，未发现统计错误。详见B3 |
| M5 · P3：单一Syntax枚举扩大无关修改面 | `parse.vt:386–425`混合38种表达式/语句/辅助节点；`name_of:1459`等窄投影仍须枚举全部变体。对应 §3–5 | 先做一个分域垂直切片，比较修改面、arena大小和自举成本，再决定推广。当前语言无enum整体通配分支，不能建议不存在的语法。详见A4 |

M1不是“全部函数加unsafe”；M4不是“禁用所有static”；M5不是“把长函数切成part1/part2”。这些改法都会掩盖真正的责任边界。

## 3. 简洁性、可读性与规模

当前Q的状态所有权已经有实质约束：输入被复制后冻结，AST/IR句柄带世代检查，IR封存后后端验证票据绑定revision并只允许一次发射，映像状态由单个owner管理。`rules.vt` 是真正无I/O/无诊断的叶层，后端不读取AST。应保留这些结构，不能为减少代码回退成公开数组和任意setter。

可读性欠缺主要在三处：错误上下文依赖隐式时序；`InitData`等数据表达尚未把不变量写进类型；大而混合的Syntax使少量有效分支淹没在无关穷尽分支中。它们比空格和文件行数更值得优先处理。

| 统计范围 | 文件数 | 物理行数 |
|---|---:|---:|
| 当前Q生产VT，不含runtime/测试 | 14 | 10,991 |
| 当前Q runtime，2 VT + 1 S0 | 3 | 220 |
| core-s/tools，14 Python + 1 Rust | 15 | 2,416 |

Q生产VT扫描到433个函数，37个物理跨度超过60行，8个文件超过500行。这些是**定位审查区域的信号，不是37项缺陷或复杂度评分**。最长跨度包括 `rv64::validate` 和 `s0::operation`（各210行），有明确分派/编码理由；本轮不因它们长而要求拆分。没有用正则嵌套深度判定架构质量。统计包括空行与注释，当前计量不是之前预算的替代口径；见 [metrics.json](metrics.json) 和 [measure_audit.py](measure_audit.py)。

排版偏离包括紧凑参数/算符、部分一行多语句、静态存储小写，以及非显然unsafe旁缺证明。这些随实质修改改善；不单独重排冻结代码，不以“所有unsafe都缺SAFETY”推定安全性失败。

## 4. 执行顺序与验收边界

1. **先补四个行为问题**：F1保证验收本身可靠；F2路径一致；F3失败残留可观察；F4诊断定位正确。每项使用能区分反例的回归，独立提交易于理解。
2. **进入K0时收紧M3和涉及的M1**：分别证明宿主桥和输出目标行为，继续用Linux作为对照。不要在完成目标适配前对用户宣称“编译器已在suba内自举”。
3. **随模块工作改善M2/M4**：具体载荷枚举、唯一统计维护者、读取快照。触及布局/调用/发射时再跑相应消费者及自举固定点；纯注释不触发全套重复测试。
4. **M5先评估一个切片**：若确实降低语法修改面再扩大。无须为内核启动把全部AST重构设成前置门槛。

## 5. 覆盖、证据和限制

- A/B并行审查覆盖当前14个Q生产文件和当前runtime的接口、存储、核心语义/失败/发射路径；不是逐条重新证明所有指令编码与语言组合。具体覆盖见 [frontend.md](frontend.md)、[boundaries.md](boundaries.md)。
- root独立检查主要代码位置，复跑四个前端错误案例与两组边界探针，并审查工具关键判定路径。对 `check_q.py`、`verify.py`、`check_next.py`、`selfhost_next.py`、`check_q_interfaces.py` 做重点阅读；其余工具检查入口/结果/断言模式。抽查P5 `run_system.py`、`check_system.py` 的显式失败及accepted记录，并未重验P5硬件行为。
- 冻结 `core-s/src` Rust种子、`core-s/bootstrap` B、`core-s/next/*.vt` P，及根目录0.5、experiments只做目录/依赖盘点，不声称完成其逐行审计。`core-s/src/lib.rs:3–9`仍引用四个根目录共享文件：`source.rs`、`diagnostic.rs`、`syntax/lexer.rs`、`syntax/unicode.rs`。不能把根目录全当无用历史删除；也不要为新规约改动冻结信任链。
- 所有运行探针在新临时目录中操作；无生产改动，因此未重跑全部回归和自举。E3身份为 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。A/B由root验收，整合报告由未参与写作的代理独立复核，记录见 [DELIVERY.md](DELIVERY.md)。

复现命令（从biomiya目录，普通Python，不启用优化；probe_asserts内部比较两种模式）：

```sh
python3 docs/reviews/style-2026-10-14/probe_asserts.py
python3 docs/reviews/style-2026-10-14/boundaries-probe.py
python3 docs/reviews/style-2026-10-14/boundaries-cleanup-probe.py
python3 docs/reviews/style-2026-10-14/measure_audit.py
```

上面命令输出证据到stdout，不会覆盖已保存证据。前端最小源、manifest和check/build命令完整保存在frontend-probes.json。
