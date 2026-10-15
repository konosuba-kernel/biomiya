# 语言参考版本

新程序从 **[S-next 0.1 参考](s-next-0.1.md)** 开始；它对应 `next/q/` 中的维护源码。B3.1 是冻结的引导语言。两者的语法和执行顺序不同，不能混用手册。

| 文档 | 对应对象 | 用途 |
| --- | --- | --- |
| [S-next 0.1 参考](s-next-0.1.md) | Q，四种固定RV64 profile | 当前实现的语法、执行行为、平台接口与实现限制 |
| [B3.1 参考](b3.1.md) | 冻结 VT 引导编译器 | 旧程序与 B/P 引导链使用；不再同步新语言 |
| [旧 0.5 参考](../../../docs/reference.md) | 根目录旧 Rust 编译器 | 历史版本，不能据此推断 Core S 支持范围 |
| [S-next 方案](../../../docs/design/core-s-next.md) | 阶段和架构目标 | 完成状态见执行 DAG，不替代使用参考 |
| [R1a 草案](../../../docs/design/reference-next.md) | 历史语言设计 | 非当前实现合同 |

## 版本身份

S-next 的 ELF 身份登记在 [s-next-0.1.json](s-next-0.1.json)，示例验证器要求哈希匹配；固定点、源码扰动和独立验收见 [执行记录](../../next/delivery.md)。Cargo 包版本不代表语言版本，也不能用任意一次成功编译冒充已登记候选。

当前S-next参考为修订4（2026-10-14），绑定K0工具链产物；核心语义不变，新增固定profile、桥和S0机器操作的使用合同。新旧ELF身份均保留，新产物的实际证据与独立判定见[维护记录](../../next/q/k0-2026-10-14.json)。

B3.1 编译器实现基线为 `51530f9`；`61d10a2` 只整理目录和验证入口，没有改变目标编译器源码。原始目标 ELF 与兼容维护产物的 SHA-256 登记在 [b3.1.json](b3.1.json)，例子验证器拒绝使用不匹配的编译器。Rust seed 的 Cargo 包版本 `0.1.0` 不能用来选择语言参考。

文档修订号与编译器版本分开。修订 1 首次集中整理当前规则，补充可运行正反例，并明确已知限制；没有添加语言能力。修订 2 登记保持语言合同的维护重构产物及其[验收依据](../refactoring.md)，保留原始 ELF 身份。后续只修改措辞、示例或更正笔误可增加文档修订号；语法、运行语义、ABI 或支持承诺改变时，应新增版本页、对应身份与例子，保留旧页供旧程序使用。构建产物变化也应重新登记身份与验收，不能跳过哈希检查冒用旧证据。

## 示例与验证

手册中带 `example:` 标记的代码是唯一的示例源码；版本 JSON 保存编译器身份、入口和预期结果，不复制程序文本。S-next 使用 `tools/check_next_reference.py` 检查三例的 check/build/运行及 check 不发布；完整语言、ABI 和 Blocking 合同由版本记录中的专项套件验证。B3.1 使用冻结的 `tools/check_reference.py`，另有拒绝、Blocking 三种结果和 FFI 完整 ELF 比较。

在 `core-s/` 下执行：

```sh
python3 tools/check_next_reference.py --compiler build/k0-p4-r2/e3.elf --record-dir build/reference-new
```

B3.1 的历史参考仍可分别验证：

```sh
python3 tools/check_reference.py --version b3.1 --compiler build/b31-refactored.elf
```

可以使用从对应源码重建且哈希匹配任一已登记产物的 C2/C3。结果保存到新的临时目录，也可用 `--record-dir NEW_DIRECTORY` 指定位置。缺少匹配编译器时按 [构建说明](../../README.md) 重建，不能跳过示例检查或把未运行记成通过。

参考示例是面向读者的可复现检查，不替代完整回归、自编译固定点或独立语义审查。
