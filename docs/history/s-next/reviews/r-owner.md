# R-Owner r1 独立验收

2026-10-13，验收者 `/root/resume_review`。未参与生产实现或正式工具编写，只读审计、制作独立 build 探针。

**R-Owner 合同通过。** SourceStore/ImageBuilder 缺口已由真实生产拥有者关闭；这不自动接受尚未完成的新固定点和整体 P4。

## 冻结身份

- 候选 `build/q-owner-r1/compiler.elf` SHA-256 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`，999,504 字节。
- 18 个声明输入集合 SHA-256 `60b44ea782a1268f30d37bf6969fc4d53cf4dbb852cbf98cee3fbc87dbeb8d65`。独立重算集合，核对所有当前输入，执行结束仍相符。
- `build/q-owner-r1/source.tar`、inputs.json 和构建日志保留。作者构建驱动为 R-ID r2，不将一次新候选构建称为完整自举验收。

## 实际结构

源码、汇编、import 表和计数迁到 input 私有存储；名称表与字节池在 model 内私有。内存客户的 add_source 同时复制路径和源码，freeze 后读取 const 视图；按值返回的 Source 不提供修改存储的入口。header 只能完成一次，仅填写 module/import 字段，不允许替换 path/bytes；对应 header 完成后不能追加 import。

符号、对象、重定位、fixup、数据和输出字节迁到 image 私有存储。具体 object/write_scalar/copy_data/relocate 接口以对象和范围校验，没有万能表 setter 或可写 slice。define 拒绝已完成符号覆盖；put 私有。begin_compile 清空整次构建，begin_text 保留前面生成的符号/静态对象，finish 后发射、数据和修补入口拒绝变更。

main 的真实顺序是 model reset → image begin_compile → input load/freeze → types/init collect → image begin_text → 函数/汇编发射 → image finish → 分析最终判定 → 发布。没有在 init 后清掉静态对象，也没有把函数 IR 交给后端修改。

终端 `model::diagnose(code, span)` 已由 parse/types 调用；check/init 通过各自节点 Span 定位语义诊断，不使用 lexer token 作为外部错误位置。私有诊断上下文不是方案禁止项。R-ID 中保存递归布局 origin 的修复继续保留。

model 剩余公开可变量为 max_ast/max_ir/max_frame 与错误状态，属于统计/终端诊断；Source/Name/Type/Decl/Image 的语义数组均无公开绕写。Decl/Symbol 等部分索引仍为受检 u32，未宣称语言提供完整借用/生命周期证明。

## 独立动态探针：16 项通过

`build/r-owner-independent-r1/probe.py`、results.json 保存 12 个运行探针；privacy.py、privacy-results.json 保存 4 个编译拒绝探针。驱动为本候选，真实导入其 model/input/image/rules/runtime，空 PATH、1 MiB guest 栈。

- 读取未 freeze 源码、重复 freeze、错误 header import 范围、活跃映像期间重开、文本阶段前发射：I_INTERNAL。
- 极大数据 offset：E_LINK，无整数溢出/panic。
- finish 后 fixup、relocate、define、copy_data：I_INTERNAL，未发布产物。
- 合法源码路径/字节快照不受原 mutable buffer 或返回 Source 值修改影响；返回 Data/Symbol 快照不修改内部表。
- 同一对象重叠 copy_data 得到独立期望字节 `[1,1,2,3,4,5,6,7]`；静态对象跨 begin_text 保留。完整生成 ELF 的 RW 数据由宿主解析 program header 后检查，实际运行生成 ELF 退出 42。
- 完成后显式开始下一次编译，数据/文本计数清空、符号重新分配，目标退出 42。
- 直接访问 input 私有 source 表计数、image 私有 data_bytes、image 私有 put、model 私有 names：即使调用处 unsafe 也 E_NAME，均定位调用者文件 7，无产物。

独立探针初稿漏写 Linux I/O 调用的 unsafe，编译器正确拒绝 E_PERMISSION；修正夹具后通过，初始记录 fixture-initial.json 保留。privacy 脚本初稿把清单绝对路径传入 CLI，被正确 E_PARSE 拒绝；改用相对路径后测试私有性。两者均不是生产缺陷。

另只读核对作者 owner 16 项与 input 24 项开发证据；它们由 R-ID 编译器生成驱动，不能充当最终新 E3 的全套回归证据。

## 后继边界

生产 14 VT 共 **10,991 物理行、325,234 字节**，比 R-ID 增加 85 行、5,816 字节；8,000 行软预算仍未达。允许 R-Bootstrap 继续，以最终 E3 重跑语言、机器、输入、Blocking、接口/容量、复制/故障、参考和 P5，并重新核对完整成本及 91 项映射。只有这些依赖和独立 Integration 通过，才能关闭整体计划。
