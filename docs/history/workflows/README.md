# 历史实施与验收记录

这里保存各阶段候选、审查、探针、日志和源码哈希。它们按当时版本冻结，文件中的路径和统计不随当前目录整理改写；`candidate` 中的状态描述的是提交验收时状态，最终判定看对应 review 与 workflow。

| 阶段 | 源码基线 | 记录 |
| --- | --- | --- |
| B0/B1、B2、B3 | `1c0f143`（包含当时累计实现） | bootstrap-prototype/、bootstrap-b2/、bootstrap-b3/ |
| B3.1 | `51530f9` | [bootstrap-b31/workflow.json](bootstrap-b31/workflow.json) |
| S-next 0.1 行为修复 | `2c7061c` + 内容哈希 | [2026-10-14交付与独立验收](behavior-fixes-2026-10-14/DELIVERY.md) |

旧脚本可能依赖当时的 `core-s/app/`、`core-s/compiler/`、文件哈希或测试数量。复现历史记录时使用对应 Git 版本；不要把历史快照脚本当作当前构建入口。运行某个旧阶段的脚本前还需核对它自己的输入合同。

当前构建与回归入口是 [core-s/README.md](../../core-s/README.md) 和 [tools/verify.py](../../core-s/tools/verify.py)。B2 输入处理试点现存于 [tests/fixtures/b2-input/](../../core-s/tests/fixtures/b2-input/)，继续服务回归测试；原始 VT 文件内容保留。
