# N3 r1 实施候选（待独立验收）

输入为已独立通过的 N2 r1。实施者为 `/root/b31_frontend`（规则、前端、image、共享 Slot 合同及整合）和 `/root/b31_machine`（rv64、machine fixture/test）。本报告不构成独立通过。

## 共同规则与静态验证

新增具体 `bootstrap::rules` 模块，只有 `model` 的判别常量依赖，不读取全局表、token、diagnostic 或 AST，不含 unsafe/static。`ScalarRule {ok,bits}` 和 `SizeRule {ok,size}` 是小值；运算接收显式 kind/数值，失败不直接输出诊断。

- literal/unary/cast/binary 的合法性和数值运算由纯函数实现；types 包装 TypeId 查表、上下文错误出口，两个常量 walker 保留各自解析/AST 遍历及定型流程。checker 的整数字面量也使用同一规则，运行期 NEG 限制保持。
- `max` 明确为宽度辅助函数：BOOL/U8/U32 有窄界，其他 kind 回 usize 上界；它自身不验证 kind。checked operations 验证其允许的 kind/表示；UNIT 比较只接受零表示。
- checked byte addition/alignment 被 types 布局包装复用。image 的 `aligned` 在调用纯计算前仍按原顺序检查 a 非零/幂次/a≤4096，再检查 n≤64MiB，保留 E_LINK/limit 的政策和报告；不对对齐后结果另加 64MiB 限制。
- `parse::zero` 更名为 `validate_static_zero` 并明确完整验证合同：即使已发现非零项仍遍历尾项；重复长度零仍验证表达式；false 不保证物理字节非零；序列化继续依赖此完整门。没有合并两个常量 walker。

## 语义 IR 与目标布局

后端将 slot 帧偏移保存到其私有表。内部 `Slot.offset` 字段直接删除，Slot 只有 ty/flags/span；没有对外稳定 ABI 需要兼容此字段。删除死字段避免同时保留无用途布局数据，Slot 由原 32 字节变为 20 字节；实际静态映像统计见组合构建证据，不只按理论净节省数宣称。

后端分支保留非法 IR 与有限类型布局检查；完成函数的 Slot/Inst/args/parameter_slots 和计数不被后端改写。机器侧变更、只读快照、反例及测试详情见 `N3-machine-report.json` 与该分支证据。前端未写 rv64/machine 文件。

## source 消费者

生产 c1/c2/c3 manifest、`tests/b3.rs` SOURCES、`tests/b3_frontend.rs` FRONTEND 和静态 BSS 测试 source 列表，以及 machine 测试 source/copy 列表均显式加入 rules。新规则直接探针使用 model/rules 两模块。旧 B3 clean 工具复制整个 bootstrap，无需更改。历史 N1/N2/B3 冻结证据不重写；本节点复现实验有独立脚本。

## 验证与区分力

- `cargo test --offline --locked --all-features --test b3_frontend`：最终 12 passed / 0 failed / 0 ignored，`N3-final-frontend-tests.log`。随后收紧纯函数 UNIT 非零表示合同并补直接探针，相关单测再通过（`N3-final-pure-tests.log`）；没有可达合法语言行为改变。
- 新增纯参数测试覆盖全部二元 op、整数宽度、checked/wrapping、除零/移位、转换/一元边界、UNIT 表示及 size/align 极值；规则边界词法审计见 `N3-rule-purity.json`。
- 双 walker 测试读取实际 CONST bits 和 STATIC 序列化字节比较预期；错误表达式分别放到 CONST、STATIC，防止第一处错误掩盖第二个 walker。另覆盖非零后非法尾项、零重复非法表达式及运行期 NEG 拒绝。
- 原 frontend 模块/Result/defer/?/循环/容量/精确诊断/函数复用用例继续通过。
- `cargo test --offline --locked --all-features --test b3_machine`：7 passed，见机器分支报告与日志；1 MiB guest 栈。其直接快照检查全部语义字段，并包含多函数机器执行和反例。
- `python3 docs/workflows/bootstrap-b31/N3-integration.py`：双方停止产品写入后，临时树从显式 manifest 输入执行 C0 全编译、一次目标自编译；PATH 为空、guest 栈 1 MiB。新一代编译器再编译独立规则/cleanup 行为程序，执行退出值应为 128/18；非法 defer、静态尾项、零长度重复均拒绝且不产生 ELF。实际命令、输入哈希、状态和映像统计见 `N3-integration-evidence.json`。
- `git diff --check` 通过。

本节点只执行一次目标自编译，不声称多代固定点或完整迁移已完成。旧 seed 启动、多代固定点、全部旧 81 项及新增项、clean build 仍由 N5 完成。未增加语言能力、通用规则框架或大数组按值上下文。
