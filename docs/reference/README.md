# 语言参考版本

新程序阅读 [S-next 0.1](s-next-0.1.md)，对应 compiler/ 中的当前实现。当前参考修订 4 描述四种固定 RV64 profile，核心语义和平台边界以正文为准。

- [版本身份](s-next-0.1.json)：示例验证器接受的 ELF 哈希。
- [冻结 B3.1 参考](../../bootstrap/frozen/core-s/docs/reference/b3.1.md)：用于 Rust/B/P 引导链，不同步新语义。
- [旧 0.5 参考](../../legacy/v0.5/docs/reference.md)：历史版本。
- [S-next 设计](../design/core-s-next.md)：方案不等同于当前功能合同。
- [K0 实施记录](../../../suba/docs/workflows/k0/DELIVERY.md)：平台实测与验收。

从仓库根目录执行：

```sh
python3 tools/check_next_reference.py --compiler build/k0-p4-r2/e3.elf --record-dir build/reference-new
python3 tools/check_reference.py --version b3.1 --compiler build/b31-refactored.elf
```

验证器检查产物身份，不能绕过哈希检查冒用旧证据。手册中的 example 标记是示例源码的唯一来源。完整复现见 [构建说明](../building.md)。
