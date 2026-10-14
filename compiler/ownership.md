# Q 的存储拥有者

R-Owner 修订将存储迁入实际写入模块。记录类型仍集中在 model；这不意味着 model 持有它们的表。

| 存储 | 拥有者 | 写入入口 | 读取与结束边界 |
| --- | --- | --- | --- |
| 源码、汇编、清单字节及文件表 | input | load 私有填充；内存客户用 begin_compile/add_source/freeze | source/assembly 返回含 const bytes 的值快照，freeze 后拒绝追加；add_source 复制路径与内容，调用者不能通过原 buffer 修改输入 |
| module/import 元数据 | input | add_import、complete_header | source/import_item 按值返回；一个文件只完成一次 header，完成后不能追加其 import；不能替换 path/bytes |
| 名称表与字节池 | model | name/name_parts | NameId 私有；get_name 返回 const bytes；S0 只构造固定 11 字节前缀，经 name_parts 入池 |
| Type/Field/Decl/Param | model | 受检追加及 complete_type/complete_decl | 具体只读查询；pending 类型/声明仅完成一次 |
| Symbol/Data/Relocation/Fixup 及映像字节 | image | begin_compile → begin_text → finish | 表与计数私有；define 一次；finish 后 word/patch/object/data/symbol/relocation 均不能变更 |
| 函数 Syntax/TypeSyntax | parse | reset/解析 | 世代 NodeId/TypeSyntaxId；结束后旧句柄失效 |
| 函数指令/槽/标签/实参 | ir | begin_function/构造/finish_ir/end_function | 世代句柄；finish_ir 封口；rv64 验证票据绑定修订并单次消费 |

`begin_compile` 是显式的新编译边界。它在声明和初始化之前清空映像表；`begin_text` 只开始机器代码阶段，保留已经生成的静态对象和声明符号。main 不再在 init 后清空这些对象。

静态初始化器只获得对象身份。`image::write_scalar`、`copy_data` 和 `relocate` 以该对象范围检查写入，不能取得可写字节 slice。机器发射只通过 word/fixup/patch_word；任意文件 offset 写入的 put 是 image 私有函数。

这些接口收拢合法写入口，不承诺阻止语言显式 unsafe 原始指针对任意内存的破坏。全局稳定 Decl/Symbol 仍是受检 u32；NameId/TypeId 和函数临时 ID 具有名义类别保证。

model::diagnose(code, span) 是语法/语义阶段提交终端诊断的统一入口。parse/types 保留各自私有的当前位置；check/init 通过语义定位提交错误，诊断不会因为解析游标移动而丢失来源。

作者回归位于 tests/compiler/owner.vt：源码与路径快照、header 一次完成、import 封口、分阶段保留静态对象、数据/patch 边界、重复 symbol 定义与最终映像封口。独立验收另见 docs/history/s-next/reviews/r-owner.md；作者回归通过不等于独立接受。
