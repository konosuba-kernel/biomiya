# A · I-r1 最终独立验收

2026-10-14；审查者 `/root/behavior_fixes_acceptance`，未参与 T/R/D/I 的实施。

**判定：Accepted。** 本判定只适用于 I-r1 冻结候选及其 T-r1、R-r1、D-r2 依赖，范围是四项行为修复及配套验证、文档。没有发现阻断该范围交付的问题。此前的 acceptance-preflight.md 只是预审，本文件才是正式组合判定。

- 候选清单：`I-candidate-r1.json`，SHA-256 `e0acbc4679dd83464fb62ebac5d9d113df506df94fc431542b77481abdd5133d`。
- 最终编译器：`core-s/build/behavior-fixes-p4-r1/e3.elf`，1,011,792 字节，SHA-256 `8051f3599704a6bf180496b14632691c3463f1239946893641f65aae21969904`。
- 只读核验脚本与证据：[acceptance-r1-check.py](acceptance-r1-check.py)、[acceptance-r1-evidence.json](acceptance-r1-evidence.json)；独立运行索引：[acceptance-r1-runs.json](acceptance-r1-runs.json)；成本复核：[acceptance-r1-cost.json](acceptance-r1-cost.json)。

## 源码与证据身份

直接重算全部 58 个候选文件哈希，开始与专项运行结束后均匹配；规约身份匹配，I-results 与 maintenance 记录逐字节相同。当前 18 项 Q 输入、聚合身份、源码胶囊逐项一致。71 项冻结种子输入未改；旧 release.json/cost.json 与基线 Git 内容逐字节相同。git diff 的修改范围为本轮预期生产文件、维护工具与文档，未修改冻结 B/P/Rust 或旧发布记录。

检查 T/R/D 候选、root 独立验收以及实际代码差异。F1 的优化保护发生在模块导入边界，不能被优化移除；F2 的共用路径检查在 syscall 之前拒绝 NUL；F3 明确保留主错误、额外 close/cleanup 错误，创建失败不清理既有输出，写失败不 chmod，close 不重试；F4 修正 parser 的字段起点并显式选用字段/记录位置，子表达式先生成指令，恢复父上下文不会重标子 IR。详见[预审的逐项合同核对](acceptance-preflight.md)。

## 自举与组合证据

未机械重跑完整自举；直接检查真实文件及已冻结完整证据，另独立执行行为抽验。

- 读取 E1/E2/E3 实际文件，完整字节一致；所有构建阶段输出哈希与实物一致，B(P) 的 E0 与 P3 候选相同，后续阶段的编译器身份顺次衔接。
- 核对五个源码/工具胶囊哈希，Q 胶囊内容等于当前 18 项来源。扰动胶囊与原胶囊仅 main.vt 不同，差异恰为统计前缀替换，匹配扰动源码哈希。
- 核对七个阶段的 stdout/stderr 实际日志与记录；重新调用已审阅的 trace_contract 检查实际 strace 的文件白名单、无 guest execve、无自读 ELF。记录均为 PATH 空、1 MiB guest 栈，成功退出。
- 独立运行最终 E3 和 perturbed.elf 对同一最小源做 check，分别实际观察原前缀与扰动前缀，均不生成输出 ELF。不是只依赖 behavior_observed 标记或两个 ELF 的差异。
- P3 记录包含 126 个语言案例和 11 个保留目标测试。最终 E3 的十套回归共 435 个场景：126 language、24 input、105 machine、48 blocking、15 limits、76 interfaces、8 copy、3 reference、12 IO、18 diagnostics。逐套核对记录哈希、编译器身份、场景数、通过标记及所记录的当前输入哈希；没有用 P3 或旧 E3 的通过代替最终 E3。
- P5 的两项程序与十个监控层反例单列。检查主记录及十二个嵌套记录哈希，实际命令退出值、反例预期退出值与结果一致。它仍是依赖宿主监控层的有限平台实验。
- 工具 guards 为 88 次入口/导入模式检查与 5 次语料工具调用。**后者只有 2 次普通模式实际执行编译器及生成程序，另外 3 次优化模式在编译前早拒绝。** 此计数不能描述成五次均完成编译运行。

只读核验共完成 673 项记录/内容/合同检查，结果保存于 acceptance-r1-evidence.json；这不是 673 个新增语言测试。

## 独立实际执行

以下命令均从 biomiya 根目录运行，使用新建证据路径，全部 exit 0：

```sh
python3 docs/workflows/behavior-fixes-2026-10-14/acceptance-r1-check.py
python3 -O core-s/tools/check_q_io_failures.py --compiler core-s/build/behavior-fixes-p4-r1/e3.elf --record-dir core-s/build/behavior-fixes-acceptance-io-r1
python3 -O core-s/tools/check_q_diagnostics.py --compiler core-s/build/behavior-fixes-p4-r1/e3.elf --record-dir core-s/build/behavior-fixes-acceptance-diagnostics-r1
python3 core-s/tools/check_q_tool_guards.py --compiler core-s/build/behavior-fixes-p4-r1/e3.elf > core-s/build/behavior-fixes-acceptance-tool-guards-r1.json
git diff --check
```

前三个专项分别为 IO 12/12、诊断 18/18（包括重新编译运行的真实 checker IR probe）、guards 88+5，通过。IO 和诊断工具直接在 -O 下运行，验证其显式 require 不依赖 assert。IO 同时检查 syscall 次序、残留文件字节/权限和完整诊断；诊断检查 check/build 的精确错误类别、文件与 UTF-8 字节位置，保留正常构造、越界退出及子 Bounds/父 Store 的区分。

这些运行属于最终 E3 的独立复核，不重复计入上述 435 场景总数。完整自举和其余套件的结果属于对实施者证据的独立审核，未声称由本审查者全部重新执行。复现须改用不存在的证据目录，避免覆盖原结果。

## 文档、规模和限制

候选 Markdown 的本地链接均能解析，正式验收链接由本文件完成；当前/历史产物已分别登记，新参考修订绑定当前 E3。独立按源字节重算成本：生产 Q 14 文件、11,018 行/326,623 字节；VT runtime 2 文件、290 行/10,615 字节；S0 19 行/345 字节；工具和新测试另列。直接解析 E3 的 ELF program headers，RW 55,135,764 字节、映像跨度 56,143,380 字节与记录一致；没有把静态预留当作峰值 RSS，也没有把最大单帧当作完整调用链栈。

保存清理仍可能失败，残留已明确报告；close 失败时文件可能已完整写入且有执行权限。没有并发路径替换安全、断电事务或持久化保证。冻结 verify.py/check_reference.py 的历史直接入口仍须普通 Python，维护入口已提前拒绝优化。验收证明 RV64 Linux-user 的修复与自举；K0、suba 内自举和完整操作系统均未实现，也不在本轮启动。

本审查者只新增 acceptance-r1* 记录和独立 build 产物，没有修改候选、旧证据、DELIVERY，没有提交。正式判定后停止写入。
