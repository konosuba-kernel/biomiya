# biomiya 规约审计 r1 独立验收

2026-10-14；审查者 `/root/biomiya_audit_acceptance`。未参与 A、B 或整合报告的作者工作。

**Accepted。** 该结论接受本次审计报告的证据、范围与表述，不表示已修复所列问题，也不替代编译器发布验收。

## 冻结身份与改动边界

- 候选：[candidate-r1.json](candidate-r1.json)，SHA-256 `6630b2405fdf77939141b39edb558a690ee729d5441ebfa5d8732b7b573f2505`。
- 主报告 SHA-256 `024b273e391945631570e273a83c4412dd263100574d564f91500e9070a978c0`；候选所列全部 17 个文件哈希独立核对匹配。
- Git HEAD 为 `2c7061cbed13959697369879e5717a8ad0c9fb21`，`git diff HEAD --stat` 为空；新增内容在审计目录中。未修改生产代码、冻结种子或历史验收证据。
- 阅读了规约、主报告、A/B 报告及三个探针源码，并核对主要源码位置。复跑所用 E3 身份与候选证据一致。没有执行全套编译器回归；本轮只新增审计资料。

## 逐项判断

| 项目 | 独立核对 | 判断 |
|---|---|---|
| F1 | 核对 `check_q.py:81–130` 和 `selfhost_next.py:180`，独立运行逐字复制工具的隔离探针。实际程序返回 0、故意期望 7；普通 Python exit 1/通过数 0，优化模式 exit 0/通过数 1 | 反例成立。报告明确这是工具配置假阳性，不撤销普通模式已有证据；P2 合理 |
| F2 | 核对 `runtime/linux.vt:35–75`，独立复跑；同一合法存活的 NUL 切片，open 拒绝且保留哨兵，remove 成功删除前缀路径 | 反例成立。公开 runtime 与 manifest 可达性已区分；未夸大成任意文件删除漏洞 |
| F3 | 核对 `input.vt:353–387`、runtime 写入/关闭路径与注入 shim；独立复跑留下 `0123456`、0755，诊断仅为 `E_IO file=0 byte=0` | 双故障证据成立。报告承认 unlink 拒绝时无法保证删除，且明确 7 字节残留不是可运行 ELF |
| F4 | 核对 `check.vt:1036–1063`、`init.vt:180–204` 与 `types::locate/fail`；四个输入各跑 check/build，八次结果均与证据一致 | 错误位置确指向前一个合法值 `11`，未知/重复字段都仍被拒绝、没有 ELF；影响限定正确 |
| M1 | 核对 `lex::decode`、`input::add_source` 和 runtime 裸切片入口；对照规约 §8 | 合同债务成立，未以缺少注释推定实际内存破坏，也未要求所有函数一律 unsafe |
| M2 | 核对 InitData、通用 add、repeat 构造和 serialize | 字段重载及运行时 kind 分派确实存在；报告明确未证明当前传错参数，要求量测枚举成本 |
| M3 | 核对 model 直接 Linux 诊断、input chmod、rv64 目标启动/退出桥 | 宿主与输出目标的区分正确，报告标为 K0 移植债务而非现有 Linux-only 功能缺陷 |
| M4 | 核对 9 个公开可变标量，以及 parse/check 和 ir/check 的重复 max 更新 | 属于维护责任问题；未发现统计错误的限定适当，P3 合理 |
| M5 | 核对 38 个 Syntax 变体及窄投影结构 | 修改面说明有依据；只建议有成本验证的切片，不把长 match 本身认作缺陷 |

F1–F4 均有可区分的行为证据；M1–M5 与行为缺陷分开。没有阻断修订项。报告保留 owner/epoch/seal 与叶层规则的已有收益，未机械套用内核 IRQ/step 约束到当前用户态编译器。修复顺序没有把全部 AST 改造设为 K0 前提。

## 计量、链接与独立证据

重新运行计量脚本，输出与冻结 `metrics.json` 完全一致；全部计量源文件哈希匹配。另用声明行扫描交叉核对 433 个函数。Q 生产 14 文件/10,991 行；runtime 3 文件/220 行；工具 15 文件/2,416 行；37 个函数物理跨度超过 60 行、8 文件超过 500 行、9 个公开可变标量。口径将 runtime 单列，统计包含空白与注释；主报告没有把这些数字解释为复杂度评分或缺陷数。REPORT/frontend/boundaries 中实际 Markdown 相对链接均可解析到现存文件。

本次新增的复核结果：

- [候选、链接、指标及前端八次调用](acceptance-r1-verification.json)
- [F1 优化模式探针](acceptance-r1-probe_asserts.json)
- [F2 NUL 路径探针](acceptance-r1-boundaries-probe.json)
- [F3 写入与清理双故障探针](acceptance-r1-boundaries-cleanup-probe.json)
- [独立计量输出](acceptance-r1-measure_audit.json)

覆盖边界仍以主报告为准：当前 Q 的结构和重点失败路径审计，不是所有语言组合/指令的重新正确性证明；历史 B/P/Rust 与 legacy 仅盘点/依赖核对，不宣称全库逐行审查。
