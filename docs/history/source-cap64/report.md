# C64：源文件容量64

2026-10-15。作者交付记录；状态：候选r1，作者验证完成，待root独立验收；不替代旧K0编译器身份。

起点biomiya `d2c6e98`；旧E3 SHA256 `049c6e094ccd044fba3b5d357583013b77f0e8e9a98d431ad919e02b0cda2144`。
需求来源：suba K3已有30 VT，K4正常模块拆分将超过32；保留模块边界，扩展工具容量。

## 范围

生产只改model/input/types三个文件：SOURCE_CAP=64；sources、headers、module_state长度均由SOURCE_CAP+1派生，为65槽（保留0）；内存输入/清单判界和header清零使用SOURCE_CAP。汇编仍8个、源码总量4MiB、清单16KiB、imports1024、prepare_depth32、guest栈1MiB，其他语言/内存规则不变。冻结B/P/Rust与历史证据未修改。

参考修订5按具体ELF哈希登记容量；旧身份仍32，不能仅按“S-next0.1”推断64支持。

## 复现入口

在biomiya根目录，所有record-dir必须为新路径：

```sh
python3 tools/check_next.py --stage P3 --seed build/layout-seed-r1/compiler.elf --record-dir build/source-cap64-p3-new
python3 tools/selfhost_next.py --p3-record build/source-cap64-p3-new --seed build/layout-seed-r1/compiler.elf --q-manifest compiler/compiler.build --record-dir build/source-cap64-p4-new
python3 tools/check_q_interfaces.py --compiler build/source-cap64-p4-new/e3.elf --baseline-compiler build/layout-p4-r1/e3.elf --record-dir build/source-cap64-interfaces-new
python3 tools/check_q_input.py --compiler build/source-cap64-p4-new/e3.elf --record-dir build/source-cap64-input-new
```

其余既有工具使用相同`--compiler`和新的`--record-dir`；完整实际命令及结果身份见[candidate-r1.json](candidate-r1.json)，大日志和逐文件输入/输出清单保留在build。check/build和生成程序均以既定1MiB guest栈运行。

## 已知边界与工程记录

扩大容量不保证任意64文件程序都能满足其他固定表或递归预算。模块依赖遍历仍递归，64模块长链/末端环专门回归；本节点不以增加prepare_depth或栈预算绕过失败。首次复用旧P3的哈希不匹配及处理见[工程记录](engineering-notes.md)。最终实测结果和成本如下。

## 回归结果

最终源码相关回归共640项通过：语言126、interfaces86、input26、limits15、machine/AST/owner105、profiles26、S0系统167、diagnostics18、blocking48、copy8、I/O失败12、参考3。完整结果在`build/source-cap64-check_*-r2/verification.json`。tool-guards通过，layout检查通过（71冻结输入身份保持）。回归使用P4-r3 E1；其字节与最终E2/E3相同，不以文件名替代编译器哈希身份。

关键新案例实际构建并运行32/33/64源程序；第64源的函数被调用；64源+8汇编的第8汇编函数与重复局部标签被执行；诊断分别定位file64与file72。65源在读不存在的overflow文件之前精确I_LIMIT拒绝，不产生ELF。旧E3对33源继续拒绝。64模块长链返回63，末端环拒绝；直接add_source两轮64项header完成与65项拒绝覆盖绕过manifest的内部入口。

## 成本

实际ELF文件1036432B，与旧E3相同；PT_LOAD文本增加260B；可写内存55137788B，比旧版55135836B增加1952B；映像跨度56169980B，比旧版增加1952B。三个表的增量分别为source记录1792B、headers32B、module_state128B，与ELF内存增量一致。均低于既定64MiB预算。自编译统计max_frame25808B是单函数最大帧，不是整条调用链或峰值RSS。详细解析结果见`build/source-cap64-cost-r1.json`。

源码实际增加两个说明注释行；没有增加生产函数或可变全局对象个数。长度、判界和重置均受同一SOURCE_CAP约束；没有引入新语法或修改冻结引导。

## 完整引导与冻结身份

`build/source-cap64-p3-r1`完成126案例及11 retained target checks；`build/source-cap64-p4-r3`完成B(P)、P(Q)、E1(Q)、E2(Q)，E1/E2/E3逐字节相等。最终E3为`build/source-cap64-p4-r3/e3.elf`，SHA256 `22d2c81fd0dbfc6de6b3b13fb2f485b1c64913849d9e59eeabe8c2757d27ebf4`。源码扰动生成不同ELF并实际改变统计前缀，通过；guest没有读取自身ELF或执行host工具，源码及工具身份检查通过。

新参考身份登记同一SHA256对应64；原三个登记身份仍32。冻结清单包含最终生产源码、变更文件、验证工具/依赖和产物摘要，完整输入/输出逐文件哈希在`build/source-cap64-evidence-inventory-r1.json`。旧P4-r1的证据不匹配和主动替代的P4-r2保留，均不计最终通过。尚未将新编译器接入suba生产，由root独立接受后协调。
