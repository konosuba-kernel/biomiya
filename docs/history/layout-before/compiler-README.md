# Core S S-next 0.1：维护源码 Q

语言使用者从 [版本参考](../../docs/reference/s-next-0.1.md) 开始；实现者从 `main.vt` 的阶段顺序开始。目录名是显式构建清单的路径，不形成自动模块搜索规则。模块名暂为 `bootstrap::*`，本目录与旧 bootstrap 不可同时选入一个清单。

## 当前K0修订（2026-10-14）

当前产物为 `core-s/build/k0-p4-r2/e3.elf`，SHA-256 `049c6e094ccd044fba3b5d357583013b77f0e8e9a98d431ad919e02b0cda2144`。完整E1=E2=E3及源码扰动通过；功能与独立验收状态见[K0记录](k0-2026-10-14.json)和[suba交付记录](../../../../suba/docs/workflows/k0/DELIVERY.md)。

K0新增四个固定profile、有限panic/trap桥、启动栈与BSS初始化、S0机器指令，以及host/target边界。Q仍运行于Linux-user；suba内宿主服务与自举尚未实现。核心语言语义不变，平台能力见参考修订4。

生产VT为11,307物理行，runtime VT另375行、S0另19行。ELF 1,036,432字节，RW预留55,135,836字节，映像跨度56,168,028字节；最大单函数frame 25,808字节。分类、源哈希和测试成本见[本轮成本](../../../../suba/docs/workflows/k0/I-cost.json)。

新增验证入口（在core-s下，记录目录必须不存在）：

```sh
python3 tools/check_q_profiles.py --compiler build/k0-p4-r2/e3.elf --record-dir build/profiles-new
python3 tools/check_q_s0_system.py --compiler build/k0-p4-r2/e3.elf --record-dir build/s0-new
python3 tools/check_q_host.py --compiler build/k0-p4-r2/e3.elf --record-dir build/host-new
```

## 前一轮行为修复（2026-10-14）

四项行为修复当时的编译器为 `core-s/build/behavior-fixes-p4-r1/e3.elf`，SHA-256 `8051f3599704a6bf180496b14632691c3463f1239946893641f65aae21969904`。E1=E2=E3；ELF 1,011,792字节，RW预留55,135,764字节，映像跨度56,143,380字节，最大单函数frame仍25,808字节。

修复验证工具优化模式假通过、NUL删除路径、保存错误与清理结果丢失、记录字段诊断错位。该修订生产VT为11,018行，VT runtime另290行、S0仍19行。证据与最终独立判定见[维护记录](maintenance-2026-10-14.json)及[组合说明](../../../docs/workflows/behavior-fixes-2026-10-14/I-summary.md)；下面保留前一基线的原始身份和统计。该修订没有新增语言特性，当时K0尚未实施。

新增回归入口（在core-s下，使用不存在的证据目录）：

```sh
python3 tools/check_q_io_failures.py --compiler build/behavior-fixes-p4-r1/e3.elf --record-dir build/io-new
python3 tools/check_q_diagnostics.py --compiler build/behavior-fixes-p4-r1/e3.elf --record-dir build/diagnostics-new
python3 tools/check_q_tool_guards.py --compiler build/behavior-fixes-p4-r1/e3.elf
```

## 2026-10-13的基线编译链

`build/snext-p4-r2/verification.json` 记录 B(P)=E0、E0(Q)=E1、E1(Q)=E2、E2(Q)=E3。E1/E2/E3 完整文件一致：

```text
7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20
```

E3 为 `core-s/build/snext-p4-r2/e3.elf`。产物 999,504 字节；目标 RW 55,135,764 字节、映像跨度 56,131,092 字节。空 PATH、1 MiB guest 栈、输入白名单及无宿主代编译检查通过。修改 `main.vt` 的统计前缀后，生成的新编译器实际报告新前缀，验证不只比较两个相同文件。源包、种子、每代日志和命令全部保存在该证据目录。

这些数值是已执行候选的证据；独立验收状态以 [DAG](../delivery.md)、[P4 r2 审查](../reviews/p4-r2.md)、[P5 r3 审查](../reviews/p5-r3.md) 和 [版本记录](release.json) 为准。

## 阅读路径

1. `main.vt`：输入 → 声明/绑定/常量/布局 → 静态初始化 → 逐函数语法和 IR → 验证 → 分析 → 发射 → 链接 → 发布。
2. `parse.vt`：具体 Syntax、TypeForm、声明头和 BodyRange；只识别语法，不查询符号和布局。头部节点持久，当前函数节点有世代。
3. `types.vt`：先绑定名字，再遍历受限常量，最后完成布局；布局不回到扫描器。完成类型不可覆盖，可以追加新的派生类型。
4. `init.vt`：语法检查为有类型 InitData；序列化只读 InitData、字节和重定位。
5. `check.vt`：值与位置、实参快照、保存调用式 defer、浅层穷尽匹配；不捕获或重放 IR。
6. `ir.vt`：具体 Instruction 和操作符载荷、SlotId/LabelId/ArgumentRange、有限表及封口。所有调用参数在实参求值后一次提交连续范围。
7. `rv64.vt`：只读语义 IR，验证后领取一次性发射票据；独立维护帧/标签/修补侧表。`image.vt`、`s0.vt` 完成机器映像和有限汇编适配。
8. `blocking.vt`：在 arena 释放前提取稳定的直接调用摘要；开启分析不改变 ELF。

`rules.vt` 是纯标量/尺寸规则；`runtime/` 是明确的 Linux I/O 和具体结果枚举，当前另有保存阶段/错误结果。没有 HIR、SSA、visitor 框架、通用上下文、借用检查或用户泛型。

## 生命周期与实际边界

驱动开启函数，解析并检查，封口 IR，验证、分析和发射，最后释放函数空间。NodeId/TypeSyntaxId 的函数世代与持久头部身份分开；SlotId/LabelId/ArgumentRange 使用 IR 世代。结束或再次复用同一个索引后，旧句柄仍会拒绝。0 是不可读取的占位，外部不能构造私有 raw 字段。

Node 为 40 B，TypeSyntax 为 24 B；Inst 为 48 B、Instruction 为 32 B，SlotId/LabelId 为 4 B。三类 I/O 结果、AST/IR、绑定/循环/清理表均使用实际具体变体；未保留 RawNode/RawInst 的兼容解包层。语义坏 IR 仍须由目标验证器拒绝，枚举本身不能证明槽类型、参数范围或标签正确。

名字与类型现在分别使用私有 NameId/TypeId，Type 的元素类型与声明身份分开。input 持有源码和清单，冻结字节后只追加一次模块头；image 持有符号、静态数据、重定位和机器字节，以 begin_compile → begin_text → finish 区分构造与封口。详细写入入口和跨阶段依赖见 [存储拥有者](ownership.md)。稳定 Decl/Symbol 仍是受检 u32；统计和终端错误字段仍有公开标量，不宣称所有全局变量均已私有化。

## 规模与复现

2026-10-13基线生产 Q 为 **10,991 物理行 / 325,234 字节**，其中非空非注释行 10,404。VT 运行库另 201 行，S0 运行适配 19 行；系统应用 228 行、平台 207 行均另行计账。原型的 8,000 行目标未达。枚举闭合分派、私有句柄、世代检查与具体载荷带来实际增加；源码没有为凑预算压行或移出未计账算法。该基线分类和哈希保留在 [cost.json](cost.json)，当前修订另见上方维护成本；`python3 tools/measure_next.py` 会按当前源码重写cost.json，复核旧证据时不要覆盖它。

从 `core-s/` 使用新的证据目录执行：

```sh
python3 tools/check_next.py --stage P3 --record-dir build/p3-new
python3 tools/check_next_trials.py --compiler build/p3-new/compiler.elf --record-dir build/trials-new
python3 tools/selfhost_next.py --p3-record build/p3-new --seed build/b31-refactored.elf --q-manifest next/q/compiler.build --record-dir build/p4-new
python3 tools/check_q.py --compiler build/p4-new/e3.elf --record-dir build/language-new
python3 tools/check_q_input.py --compiler build/p4-new/e3.elf --record-dir build/input-new
python3 tools/check_q_machine.py --compiler build/p4-new/e3.elf --record-dir build/machine-new
python3 tools/check_q_blocking.py --compiler build/p4-new/e3.elf --record-dir build/blocking-new
python3 tools/check_q_limits.py --compiler build/p4-new/e3.elf --record-dir build/limits-new
python3 tools/check_q_interfaces.py --compiler build/p4-new/e3.elf --record-dir build/interfaces-new
python3 tools/check_q_copy.py --compiler build/p4-new/e3.elf --record-dir build/copy-new
python3 tools/check_next_reference.py --compiler build/p4-new/e3.elf --record-dir build/reference-new
python3 next/system/check_system.py --compiler build/p4-new/e3.elf --out build/system-new
```

P3 的验收工具固定 P，Q 的工具按指定 ELF 测试真实 Q 源码；不得以 P 后端测试替代 Q 的变体 IR 验证。B 的重建仍用 `tools/verify.py`；`seed.json` 会核对身份。独立验收不由脚本自行宣布。

系统样本在真实 QEMU 设备/双 hart 上运行，平台监控层和宿主 LLVM/QEMU 依赖单列成本。当前没有 suba 内自举或完整 OS 的声明。
