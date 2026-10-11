# 独立验收探针归档

这些是 `/root/acceptance` 的独立验证代码，不属于产品实现或既有测试行数。`archive-map.json` 把 E/B/P 判定中的临时路径映射到保留的源码及摘要。原始 Cargo 清单单独保存；可运行清单仅把依赖路径改为仓库内的相对路径。

在 biomiya 仓库根目录执行：

```sh
cargo run --offline --manifest-path docs/workflows/bootstrap-prototype/probes/machine/Cargo.toml
cargo run --offline --manifest-path docs/workflows/bootstrap-prototype/probes/pipeline/Cargo.toml
cargo build --offline --manifest-path core-s/Cargo.toml --all-features
python3 docs/workflows/bootstrap-prototype/probes/integration/probe.py
```

machine 探针验证常量位模式、PCREL 边界以及类型化 CFG 到直接 ELF；pipeline 探针验证阶段诊断保留、4 MiB 边界和实参快照顺序。integration 探针验证从长源程序开始的两条执行路径、Unicode 入口、失败发布与文件别名保护、必要源文件的独立离线构建和规模复算。

运行需要 Cargo 缓存，以及主原型测试使用的 QEMU、Clang、LLD；integration 另需 Python 3 和 cloc。缺工具即失败。integration 锁定 I r1 的输入摘要；将来修改产品后应建立新的验收版本，不能静默把旧记录当成新版本证据。
