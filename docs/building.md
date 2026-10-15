# 构建与验证

命令均在 **biomiya 仓库根目录**执行。需要 Python、Rust/Cargo 缓存、qemu-riscv64；机器与系统回归还使用 LLVM 和 qemu-system-riscv64。不要启用 Python 优化模式。

## 完整引导

```sh
python3 tools/check_layout.py
python3 tools/verify.py --record-dir build/rebuilt-b
python3 tools/check_next.py --stage P3 --seed build/rebuilt-b/compiler.elf --record-dir build/p3-new
python3 tools/selfhost_next.py --p3-record build/p3-new --seed build/rebuilt-b/compiler.elf --q-manifest compiler/compiler.build --record-dir build/p4-new
```

证据目录必须是新路径。已有身份匹配的 B 可以替代重建产物，其 SHA-256 为 90c9523de43bebbd0a6932e74abc8cd670b60478c3b4f0fc1eff74fbf4cfb686。迁移前的 P3 记录包含旧源码路径，须重新运行 P3 后才能对新树做当前源码验证。

## 使用

```sh
mkdir -p build
qemu-riscv64 -s 1048576 build/p4-new/e3.elf check compiler/compiler.build
qemu-riscv64 -s 1048576 build/p4-new/e3.elf build examples/hello/program.build
qemu-riscv64 build/hello.elf
```

示例退出 42。build 拒绝覆盖已有目标；check 不发布 ELF。当前编译器清单默认输出 build/snext-q.elf。

## 专项回归

```sh
python3 tools/check_q.py --compiler build/p4-new/e3.elf --record-dir build/language-new
python3 tools/check_q_machine.py --compiler build/p4-new/e3.elf --record-dir build/machine-new
python3 tools/check_q_input.py --compiler build/p4-new/e3.elf --record-dir build/input-new
python3 tools/check_q_copy.py --compiler build/p4-new/e3.elf --record-dir build/copy-new
python3 tools/check_q_io_failures.py --compiler build/p4-new/e3.elf --record-dir build/io-new
python3 tools/check_q_diagnostics.py --compiler build/p4-new/e3.elf --record-dir build/diagnostics-new
python3 tools/check_q_blocking.py --compiler build/p4-new/e3.elf --record-dir build/blocking-new
python3 tools/check_q_interfaces.py --compiler build/p4-new/e3.elf --record-dir build/interfaces-new
python3 tools/check_q_limits.py --compiler build/p4-new/e3.elf --record-dir build/limits-new
git show 0460ad7:core-s/next/q/model.vt > build/pre-host-model.vt
python3 tools/check_q_host.py --compiler build/p4-new/e3.elf --record-dir build/host-new --baseline-model build/pre-host-model.vt
python3 tools/check_q_profiles.py --compiler build/p4-new/e3.elf --record-dir build/profiles-new
python3 tools/check_q_s0_system.py --compiler build/p4-new/e3.elf --record-dir build/s0-new
python3 tools/check_next_reference.py --compiler build/p4-new/e3.elf --record-dir build/reference-new
python3 tools/check_q_tool_guards.py --compiler build/p4-new/e3.elf
python3 experiments/system/check_system.py --compiler build/p4-new/e3.elf --out build/system-new
```

参考验证要求 ELF 身份已登记，不能跳过身份检查。工具完整选项见 --help。机械验证不替代独立语义验收。编辑器插件独立构建，见 [插件说明](../editors/vscode/README.md)。仓库根目录不再放旧 Cargo 项目。
