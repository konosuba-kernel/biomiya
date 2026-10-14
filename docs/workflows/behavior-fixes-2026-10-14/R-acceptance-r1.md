# R-r1独立验收

2026-10-14；审查者/root，未参与R代码实施。判定：Accepted。

核对candidate所有hash、runtime copy_path/open_name/save_new及input::save、诊断打印、故障shim与测试判定。NUL验证对open/remove/save共用；仅成功创建后允许close/unlink，写失败跳过chmod，最早phase/errno保留，close恰好一次，清理失败报告residual；公有旧I/O错误码未变。private copy_path的97字节目标前提在所有调用点成立。没有新增通用框架、事务承诺或其他公共切片API迁移。

独立执行 `python3 core-s/tools/check_q_io_failures.py --compiler core-s/build/snext-p4-r2/e3.elf --record-dir core-s/build/behavior-fixes-R-root-r1`，12例通过，实际修改模块由E3编译后运行；证据R-root-verification.json。源hash与R-r1一致。检查注入发生在input读取完毕后，close错误先执行真实close；事件/磁盘内容/诊断三种观察相互约束，失败不会仅由被测返回值自证。

write+unlink残留7字节0600；chmod/close双故障保留内容及主/次错误的限定与报告一致。失败write后未chmod不能推出所有失败都无可执行文件（close失败可能发生在完整write/chmod后），测试和报告已明确区分。主错误以及cleanup errno非零都可见。无法unlink时不承诺删除；没有覆盖突然断电或并发替换输出路径，save_new已说明独占路径前提。

既有input24/copy8由作者执行，记录绑定真实模块hash，作为补充，不以旧E3接口76例冒充新编译器验证。组合新E3验证由I执行。新测试工具用显式require，不依赖assert。
