# Core S 实施前封存记录

本目录逐字保存 Core S 语义设计与成本估算的 DAG、候选及独立审查，包括失败版本。`manifest.json` 提供归档内每个文件的 SHA256。

- `core-s/` 原位置：`build/simplification/core-s/`。
- `core-s-cost/` 原位置：`build/simplification/core-s-cost/`。
- 记录中的路径和哈希保留历史原值；它们是历史证据标识，不表示实施后文件仍有相同内容。
- 源实现基线：`5301ce5a0225127c45e33afcacd29157d38655f9`，语言与编译器仍为 Core 0.5。
- 当前封存提交只增加设计、估算与归档，不宣称 Core S 已实现，也未为文档封存重复运行产品测试。
- 既有 Core B 实验保留为历史，不作为 Core S 编译路径或净缩减成果。

用户已要求封存当前版本并推进 Core S。后续在 `codex/core-s` 分支先完成有界架构原型；原型验收后按实测规模复评完整迁移，不能把原型当作完整 Core S 交付。
