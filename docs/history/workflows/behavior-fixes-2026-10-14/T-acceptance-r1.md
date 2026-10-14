# T-r1独立验收

2026-10-14；审查者/root，未参与T代码实施。判定：Accepted。

核对11维护模块：拒绝分支位于导入verify/check_reference/check_next之前，使用不可被-O移除的if，普通main/导入路径不改变；没有修改seed.json或71个冻结来源。工具中断在创建证据目录前发生。selfhost原main-only保护提升到模块入口，不能通过直接导入绕过。

独立执行 `python3 core-s/tools/check_q_tool_guards.py --compiler core-s/build/snext-p4-r2/e3.elf`，保存T-root-verification.json：88入口/导入模式检查与5个真实编译语料执行通过。真实返回0的程序，正确期望通过、期望7拒绝；-O/-OO/环境优化早拒绝。结果记录中的全部工具hash与T-r1当前候选一致。git diff --check通过。

冻结verify.py/check_reference.py不在维护入口修复范围内，直接优化调用仍是历史限制；用户应使用普通Python。该限制已明确记录而非隐瞒或破坏种子身份。新增工具使用显式require，不用assert作通过判定。此验收不代替I对当前组合源码与新E3的回归。
