# T-r1：维护中的验收工具拒绝 Python 优化模式

2026-10-14；作者 `/root/fix_verification_tools`；基线
`2c7061cbed13959697369879e5717a8ad0c9fb21`。候选冻结清单见
[T-r1-candidate.json](T-r1-candidate.json)，执行结果见
[T-r1-verification.json](T-r1-verification.json)。等待 root 独立验收，作者停止写入。

## 修改与责任边界

11 个维护工具在模块导入阶段用显式 `if not __debug__: raise SystemExit(...)`
拒绝 `-O`、`-OO`、`PYTHONOPTIMIZE`，在导入其他验收模块、解析命令行和创建证据之前退出。
这同时约束脚本直接调用与作为模块导入的调用路径，避免只在 `main()` 防护而遗漏导入函数。
没有替换各处断言、放宽测试结果或输入身份检查，也没有新增共享依赖。

涉及 check_next、check_next_reference、check_next_trials、check_q、check_q_blocking、
check_q_copy、check_q_input、check_q_interfaces、check_q_limits、check_q_machine、selfhost_next。
最后两个依赖冻结 verify.memory 的断言；selfhost_next 原本在 main 内的检查上移到导入前。
各模块的短保护重复是有意的：入口自身完整表达运行条件，现有证据依赖闭包无需引入新模块。

**冻结边界：**verify.py 与 check_reference.py 被 next/seed.json 纳入 B3.1 的 71 项来源哈希，
保持原字节。本候选没有修补它们被直接以优化模式运行或直接导入时的历史缺陷；历史工具必须使用
普通 Python。维护工具在导入它们之前拒绝优化，故不存在经当前维护入口绕过的路径。
measure_next 没有依赖断言的验收，不被无故禁止优化。

## 验证

从 biomiya 目录执行：

```sh
python3 core-s/tools/check_q_tool_guards.py --compiler core-s/build/snext-p4-r2/e3.elf
git diff --check -- core-s/tools
```

两条命令均 exit 0。第一条 stdout 保存为 T-r1-verification.json；第二条无输出。
新自测使用明确条件和异常，不依赖 assert；真实编译器 SHA-256 为
`7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。

- 11 个模块 × 脚本/导入两种入口 × 普通/-O/-OO/PYTHONOPTIMIZE 四种模式，共 88 次检查。
  普通模式可正常导入和显示帮助，其余模式全部以专用拒绝信息退出。
- 复制原工具到新临时目录，只替换语料：实际返回 0 的程序在普通模式期望 0 时通过；
  故意期望 7 时失败并记录 passed=0。两个正常模式均实际完成 check、build、QEMU 运行，
  最后真实退出码均为 0，证据保留各条命令。
- 同一错误期望在三种优化方式下均被早拒绝，尚未创建指定证据目录；不能再记录假通过。
- 所有 71 项冻结来源哈希匹配；测试前后维护工具哈希相同。

此处不是完整发布套件重跑。该候选只修改工具运行条件；正常工具的完整集成回归由 I 节点执行。
自测列表是当前 11 个维护入口的明确清单，新验收入口若依赖断言，也需加入此运行条件与清单。
