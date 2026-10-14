# 引导链

当前编译器位于 [compiler/](../compiler/README.md)。本目录保留从 Rust 到当前 S-next 的引导材料。

- `frozen/` 保留 B3.1 原内部路径。`core-s/src/` 是 Rust 种子，`core-s/bootstrap/` 是 B3.1 VT 编译器，`core-s/runtime/` 是其运行支持。
- `frozen/src/` 的四个文件是种子直接依赖的冻结副本。旧 0.5 的独立副本在 `legacy/v0.5/src/`；冻结树不依赖该旧项目的可变源码。
- `transition/` 是过渡 P：`.vt` 字节保留，构建清单改用新路径。
- `transition/seed.json` 原样保留，71 个 source_sha256 路径均相对于 `frozen/`。其中的 rebuild 字段仍描述历史命令。

从仓库根目录执行 `python3 tools/verify.py --record-dir build/rebuilt-b`。顶层入口带 Python 优化模式保护，调用 frozen 中原字节验证器。后续命令见 [构建说明](../docs/building.md)。不要直接以 `-O`、`-OO` 或 `PYTHONOPTIMIZE` 运行冻结工具。
