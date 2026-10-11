# N2 r1 实施候选（待独立验收）

输入 N1 r1：独立判定见 `N1-r1-review.json`。作者 `/root/b31_frontend`；不自行宣布独立通过。

## 状态与接口

- `types::token` 仅是扫描游标；`types::diagnostic` 是独立的小 `Span`。`seek`、`advance`、拆分右尖括号会同步扫描诊断；语义操作用 `locate` 改诊断，不移动游标。`span()` 仍表示扫描位置，`fail` 与带位置的语义容量错误使用诊断位置。
- `types::prepare` 递归进入另一个声明前保存 Token 和 diagnostic Span，完成后同时恢复。没有复制 headers/fullnames/preparing 等定长全局表。
- checker 的 `location` 只设置独立诊断和 IR provenance，不写 token。AST arena 用 `parse::reset(file)` 明确 FileId；静态常量/零值判定/序列化遍历按节点定位，语义名字解析也使用 arena FileId。
- 每个函数的初始化唯一由 `check::function` 负责：model 的 slot/inst/args/parameter counts，parse 的 Node arena/file/depth，IR 的 labels/capture/location，以及 checker 的 bindings/loop/defer/depth 等小状态。主 driver 和 frontend probe 不再重复 reset。
- model 全局类型/声明/参数/数据/符号表贯穿编译；types 全局声明准备表贯穿编译；model 的函数表和 parse/IR/check 工作表逐函数复用；rv64 仍拥有自身 labels/type traversal/frame 工作表，本节点不改 N3 的帧偏移所有权。
- driver 的源函数阶段末清理，以及静态初始化阶段末 `parse::reset(0)`，均明确说明清理目的；它们不是重复的函数入口初始化。统计峰值继续累计。

## 验证

从 `core-s` 执行：

- `target/debug/core-s project build/n2-final-c0.build`：成功。该临时清单是原 c1.build 仅替换 output，输出 `build/n2-final-c0.elf`；生产 manifest 未修改。
- `cargo test --offline --locked --all-features --test b3_frontend`：最终 **9 passed / 0 failed / 0 ignored**，见 `N2-focused-tests.log`。包含原 7 项及新增两项。
- 通用 frontend probe 检查每个函数结束后六个 Token 字段仍等于块后的真实词素，并检查诊断 FileId。专门复用用例通过显式 probe 参数重复检查同一函数，比较 slots/IR/args/labels/parameter counts；避免在整套自编译源码探针中重复一遍所有函数。
- 复用用例含多层前向 struct/const、参数、数组、调用、循环、break、defer、unsafe，再接小函数；原有用例继续覆盖 Result/?/continue 和递归布局拒绝。
- 精确诊断断言包括第二文件中的未定义名字、类型错误、权限错误、静态数组元素错误、递归布局、递归布局返回后的容量错误和前向常量类型错误。

从仓库根执行 `python3 docs/workflows/bootstrap-b31/N2-probes.py` 可重现 `N2-final-probe-evidence.json`。脚本只改变临时副本，测试区分力如下：

| 输入/变异 | 正常结果 | 故意遗漏或污染的结果 |
| --- | --- | --- |
| 递归准备 Inner 后 Outer 数组超限 | I_LIMIT byte=32 | 不恢复 diagnostic：byte=51 |
| 前向 const A 引用 B，返回后类型不符 | E_TYPE byte=13 | 不恢复 diagnostic：byte=33 |
| 函数检查后的扫描 Token | probe exit=0 | 模拟 checker 写 token：exit=9 |

C0 全编译、上述目标探针均成功；guest 栈固定 1 MiB。`git diff --check` 通过。没有新增模块、改 Rust seed 或语言能力，没有把大数组放入按值传递的上下文。

本节点不重复全 81 项、旧 seed 完整迁移或 clean build；这些依照派发留给 N5。没有已知未解决的 N2 测试失败；N3 的规则重构、语义 IR 只读与目标帧表尚未实施。早期与最终日志分别保留，不覆盖 N1/B3 证据。
