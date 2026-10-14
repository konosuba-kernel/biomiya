# 数据表示回归样本

这里保留具体变体与整数标签两套对照程序，以及类型误用、私有字段和过期句柄的负例。cases.json 描述 7 个用例，验证器还检查系统文件回滚样本。

从仓库根目录执行：

```sh
python3 tools/check_next_trials.py --compiler build/p3-new/compiler.elf --record-dir build/trials-new
python3 tests/trials/measure.py > build/trial-measurements.json
```

measure.py 只统计源码和预测布局，不能替代目标执行。原试点设计、测量和迁移评估保存在 [历史目录](../../docs/history/s-next/trials/)，自编译操作见 [构建说明](../../docs/building.md)。
