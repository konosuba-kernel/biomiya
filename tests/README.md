# 当前回归输入

- language/：S-next 语言用例。p2、p3 保留迁移阶段身份；当前 Q 使用 p3 集合并检查 p2 用例仍被覆盖。
- compiler/：当前 Q 的 AST、IR、机器码、I/O、诊断、目标 profile 和宿主桥测试。
- trials/：具体类型和句柄表示实验，用于过渡 P 的兼容验证。

从仓库根目录运行 [tools/](../tools/README.md) 中的验证器。系统实验位于 [experiments/system/](../experiments/system/README.md)。冻结 Rust/B3.1 回归在 bootstrap/frozen/core-s/tests/，旧 0.5 回归在 legacy/v0.5/tests/，不与当前接受集合并。
