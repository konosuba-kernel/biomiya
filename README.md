# biomiya

konOSuba 的教学系统语言与自举编译器。当前工作实现是 **Core S B3.1**，优先级为自举、简单、现代；已经在 RV64 Linux-user 下实现多代自编译固定点。

## 当前入口

| 目录 | 职责 |
| --- | --- |
| [core-s/bootstrap/](core-s/bootstrap/) | VT 编写的当前编译器：前端、IR、RV64、汇编适配、ELF 和可选 Blocking 分析 |
| [core-s/src/](core-s/src/) | 启动自举链的 Rust 种子编译器 |
| [core-s/runtime/](core-s/runtime/) | 当前 Linux 运行支持 |
| [core-s/tests/](core-s/tests/) | 回归测试；早期 B2 扫描与复制程序收在 fixtures/b2-input/ |
| [core-s/tools/](core-s/tools/) | 当前源码树的干净构建与自举验证入口 |

请从 [Core S 构建与使用](core-s/README.md) 开始。当前实现仍是有明确限制的语言子集；Linux-user 自编译不代表 SOS 内自举。尚无与 B3.1 完整对应的统一语言参考手册，已实现范围及文档关系在上述说明中列明。

## 保留的历史内容

根目录的 `src/`、`tests/`、`Cargo.toml`、`runtime/`、`platforms/`、`examples/`、`tools/` 属于旧 0.5 实现；在仓库根目录执行 Cargo 命令会构建旧编译器。当前 Rust 种子仍直接复用 `src/source.rs`、`src/diagnostic.rs`、`src/syntax/lexer.rs`、`src/syntax/unicode.rs` 四个基础文件，所以尚不能直接删除整个旧源码树。

- [0.5 参考](docs/reference.md)、[目标补充](docs/architecture-rv64.md)、[旧构建说明](docs/building.md) 对应旧实现。
- [R1a 参考草案](docs/design/reference-next.md) 是设计目标，不能当作当前功能清单。
- `experiments/` 保存早期实验，不参与当前自编译。
- [历史验收记录](docs/workflows/README.md) 按当时源码版本冻结；其中的旧路径不代表当前目录布局。

旧 0.5 基线封存在 `8feff82`；B3 基线为 `1c0f143`；B3.1 实现及整理前目录为 `51530f9`。历史证据保留原字节与哈希，当前构建入口不放在历史验收目录中。
