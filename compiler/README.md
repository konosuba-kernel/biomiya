# 当前 S-next 编译器

本目录是维护中的编译器 Q。[compiler.build](compiler.build) 的路径相对于仓库根目录。模块名暂保留 bootstrap::*，目录移动不改变模块身份或语言语义。

阅读顺序：

1. main.vt：装载清单、声明与布局、逐函数检查与发射、链接与保存。
2. lex.vt、parse.vt：词法、具体语法树及源码范围。
3. types.vt、init.vt：名字绑定、常量、布局与静态初始化。
4. check.vt、ir.vt：语义检查、调用式 defer、具体 IR 与函数空间生命周期。
5. target.vt、rv64.vt、s0.vt、image.vt：固定 profile、机器码、有限汇编、重定位与 ELF。
6. blocking.vt：可选直接调用阻塞分析。
7. runtime/：字节与 I/O 支持、Linux 宿主桥。

存储职责见 [ownership.md](ownership.md)。内部合同测试位于 [tests/compiler/](../tests/compiler/)，语言用例位于 [tests/language/](../tests/language/)。构建与固定点命令见 [构建说明](../docs/building.md)。旧发布与成本记录位于 [历史目录](../docs/history/s-next/compiler/)，重新测量写入 build/cost.json。

当前K0产物身份与边界见[工具链记录](../docs/k0-toolchain.json)。生产VT为11,307物理行，运行支持另375行VT及19行S0；固定点和成本按实际内容哈希记录，历史成本不覆盖。
