# D-r2：记录构造诊断的来源位置

2026-10-14；实施者 `/root/fix_diagnostic_spans`。基线为 `2c7061cbed13959697369879e5717a8ad0c9fb21`。本文件是节点作者交付说明，独立验收由 root 完成；不代表完整组合的自举验收。

## 修复范围与理由

- `parse.vt::make_expression` 在读取每个字段标识符之前保存当前 token 的字节起点，赋给对应 `FieldValue`。整个 `make` 与字段值表达式继续保留各自原有 span。
- `check.vt::struct_value`、`init.vt::build_record` 显式接收记录 span。未知、重复、不可见字段直接使用当前 FieldValue 的 span 诊断；遗漏字段和整体类型错误使用记录 span。错误类别与拒绝边界不变。
- 递归字段值检查成功返回后，恢复记录的诊断位置；局部构造同时恢复 IR 的当前位置，再生成构造操作。子表达式已经生成的指令不被重新标记，子表达式内部失败也在恢复之前报告。

没有新增公共接口、改变 AST 表示或引入通用错误框架。`types::diagnostic` 仍是已有诊断上下文；此次只消除记录字段错误对前一个子表达式位置的隐含依赖。

## 修订依据

首个候选只修改 check/init，真实测试结果为 8/15：遗漏、整体类型、子表达式错误和 IR probe 正确，字段错误仍指向整个 `make`。原因是 parser 原先把每个 FieldValue 的起点也设成了整个构造的起点。root 批准把 D 的写入范围扩展到该解析函数的两行修复。失败的 [D-r1-tests.json](D-r1-tests.json)、候选 hash 和构建记录保留，不覆盖也不把失败测试改成通过。

## 验证合同

`core-s/tools/check_q_diagnostics.py` 对命名编译器执行真实 check/build，不使用可被 `-O` 删除的 assert。每次使用新证据目录，保存编译器、工具、测试输入及哈希；IR probe 再捕获完整依赖源码。拒绝例检查精确错误类别、文件号、UTF-8 字节偏移，并检查没有 ELF 输出。

18 项包括：

1. 局部/static 的未知字段、重复字段四个原始反例；冻结 E3 上重新执行的八次命令仍复现旧定位，见 [D-baseline.json](D-baseline.json)。
2. 局部/static 的跨模块私有字段、遗漏字段、字段值类型错误。
3. 局部记录整体类型不匹配，局部/static 嵌套记录之后的未知字段。
4. 局部/static 在中文注释及变长前字段表达式之后的字段错误，预期位置用 UTF-8 字节索引计算。
5. 正常 static 与局部构造，生成程序退出 31；字段表达式中的数组越界仍退出 101。
6. 执行真实 checker 的 IR probe：字段内部 Bounds 指令仍属于下标表达式，两条构造 Store 属于整个记录。运行时 panic 目前只退出 101，因此没有虚构运行时文本位置检查。

另以无效 ELF 充当编译器在 `python3 -O` 下运行工具，要求工具失败；这个负控检查验收工具本身，并不作为语言回归计数。

## 复现与边界

用冻结 E3 编译完整 D 源码快照，再用生成的候选编译器运行上述回归。快照中 check/init/parse 来自 D-r2，其他源取基线 HEAD，以隔离并行 R 修改。构建输入身份见 [D-r2-build.json](D-r2-build.json)。不使用旧 E3 的行为冒充新候选测试结果。

```sh
python3 -O core-s/tools/check_q_diagnostics.py \
  --compiler core-s/build/behavior-fixes-D-r2/compiler.elf \
  --source-root core-s/build/behavior-fixes-D-r2 \
  --record-dir core-s/build/behavior-fixes-D-r2-tests
```

证据目录必须不存在。编译器/IR probe 的大文件和完整源码快照保留在 ignored 的 `core-s/build/`，本目录保留精简 JSON 结果与哈希。D 节点不修改旧发布记录，不宣称完成 T/R/D 组合的固定点或全部语言回归；这些由 I 节点完成。

验证结果：18/18 通过，共 38 次实际命令；其中 15 个拒绝用例各执行 check/build，2 个正常编译用例各执行 check/build/运行，IR probe 编译并运行。完整结果见 [D-r2-tests.json](D-r2-tests.json)。优化模式无效 ELF 负控为 0/18、工具 exit 1，见 [D-r2-optimized-reject.json](D-r2-optimized-reject.json)。

候选 ELF SHA-256：`ae3a7caf8cc7aeed0ebf882c4025b22bb691a33c70dbe817b04c831322083a42`。生产源与构建快照逐字节一致；`git diff --check` 与 Python 语法编译通过。冻结清单见 [D-r2-candidate.json](D-r2-candidate.json)。实施结束，停止写入，等待 root 独立验收。
