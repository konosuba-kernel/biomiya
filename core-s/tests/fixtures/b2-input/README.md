# B2 输入处理回归夹具

此目录保留早期 Source、扫描器、清单解析和复制应用，供 `tests/b2.rs` 与 `tests/b2_runtime.rs` 验证读写、短 I/O、EINTR、词法错误及失败清理。它不参与当前编译器自编译；真正的目标编译器位于 `bootstrap/`。

原位置是 `core-s/compiler/` 和 `core-s/app/`，迁移前版本保存在 Git `51530f9`。VT 模块名字保持 `compiler::*` / `app`，避免给夹具引入无关语义变更。

在 `core-s/` 下单独运行（输出必须尚不存在）：

```sh
cargo build --offline --locked --all-features
mkdir -p build
target/debug/core-s project tests/fixtures/b2-input/copy.build
qemu-riscv64 build/b2-copy.elf tests/fixtures/b2-input/copy.manifest
```

`copy.build` 构建测试应用；`copy.manifest` 是应用的运行输入，拼接三个夹具模块，输出 `build/b2-sources.vt`。

## 历史试点的行为约定

首版固定上限：清单 16 KiB，source 32 项，assembly 8 项，路径 96 字节，单源 64 KiB。清单缓冲和一个复用的源缓冲位于 BSS；无需堆分配。路径检查拒绝空白、控制字符、反斜线、绝对路径、空分量及 `.`／`..`。目标清单以 ASCII 空白分隔指令／参数，路径内也拒绝 Unicode 空白的 UTF-8 编码。两者均不构成符号链接安全沙箱。视图在 unsafe 访问期间必须保持有效。

复制程序退出码：0 成功；10 argv；11 路径；12 打开／已有输出；13 关闭；14 删除；15 读取；16 写入／零进展；17 缓冲上限；20 清单；21 扫描。扫描器库保留具体字节位置，当前命令行只返回分类退出码。输入读取无论成功或失败都会关闭描述符；关闭失败会作为失败处理，已有读取错误优先。输出关闭失败同样清理新输出。

`tests/b2_runtime.rs` 用真实 RV64 执行检查成功和失败路径。短 I/O provider 只替换系统调用边界，强制每次最多 7 字节并各注入一次 EINTR；复制仍由同一套目标源码完成。
