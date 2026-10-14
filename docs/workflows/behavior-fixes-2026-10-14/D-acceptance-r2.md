# D-r2独立验收

2026-10-14；审查者/root，未实施D生产代码。判定：Accepted。r1真实失败保留；本判定仅适用于r2。

核对冻结files哈希均匹配。parse在ident前取token字节起点，只改变FieldValue；check/init字段校验直接使用该span，整体/遗漏错误用record span；子expr先完成、父location后恢复，未重标已生成IR，也不会截获子expr自身错误。

root单独构造临时目录，读取冻结17个案例并执行check/build及两个正常运行（36命令），全部通过，D-root-verification.json。该检查直接核对UTF-8字节偏移、错误码/文件号、无失败输出以及正常返回31/越界101，编译器SHA与D-r2相符。另审查真实checker IR probe的完整源/编译输入与作者记录，在biomiya根目录执行 `qemu-riscv64 -s 1048576 core-s/build/behavior-fixes-D-r2-tests/ir_ownership/probe.elf` exit0：子Bounds与外层Store位置保持区分。I还将以最终E3重编/重跑该probe。

作者完整18项/38命令与root结果一致。新增验证器使用显式require和非空/唯一案例检查；不依赖assert，-O负控拒绝无效编译器。范围仅为记录构造定位，不宣称解决所有递归诊断上下文或已完成组合自举。
