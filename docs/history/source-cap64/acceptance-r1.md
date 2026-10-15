# C64-r1 独立验收：通过

2026-10-15。验收者`/root`；作者`/root/ram_fdt`。绑定候选`candidate-r1.json` SHA256 `edaf2e50a13a117788404e0b19280b2a1591d52abca200c1a1ed03cc94d82699`，编译器E3 SHA256 `22d2c81fd0dbfc6de6b3b13fb2f485b1c64913849d9e59eeabe8c2757d27ebf4`。本记录是独立判定；作者报告和candidate的“待验收”状态保留为冻结时快照。

## 核对与独立验证

- 审查3个生产文件：三张表、manifest与内存输入判界、header清零均由SOURCE_CAP派生；module_state按实际source数量初始化。未改冻结B/P、prepare_depth32、8汇编、其他容量和1MiB栈。
- 核对63个直接源码/工具/依赖/证据身份，以及完整2987项证据文件清单（55,102,667字节），均与冻结摘要一致。检查P3/P4记录、640项回归、扰动观察及E1/E2/E3完整ELF相等。没有把作者机械验证字段改成独立通过，也没有宣称独立重跑整条引导链。
- [independent-r1.py](independent-r1.py)不导入作者测试模型，新增5项真实RV64运行/拒绝验证：64个随机排列文件逐一贡献不同类型、定长数组、私有字段和函数；旧编译器拒绝同一输入；65文件在读缺失文件之前精确拒绝且保留既存输出；第64文件仍不能zero构造其他模块的私有字段；64个同模块文件的私有常量全被实际使用。通过，原始记录`build/source-cap64-root-cases-r1/verification.json`。
- 新编译器重新编译已接受的K2冻结源快照，生成内核完整ELF与原接受产物**逐字节相同**，SHA256 `76d0452c54b9d3542eb5f0e5e94dfd9a1e63bc7fada1550abf4b50ed30c3de7c`。记录`build/source-cap64-root-consumer-r1/verification.json`。没有据此声称尚未冻结的K3或K4已通过。
- 独立解析ELF program headers：text增加260B、RW/映像跨度增加1952B，文件字节数不变。与3表推算一致，仍在64MiB映像预算。成本记录`build/source-cap64-root-review-r1/cost.json`。
- 执行前后重新核对冻结文件，`git diff --check`通过。测试在K3隔离时延测量窗口结束后执行，没有混入该窗口的宿主并发负载。

## 判定边界

C64满足容量扩容、旧边界负控、全部表/重置、完整自举身份及相关回归要求，可以作为后续suba阶段的工具链输入。64源文件不豁免其他资源、递归或栈限制；当前没有新增语言语义、优化器或异步能力。K3已有候选仍使用旧编译器，后续集成须明确采用哪个版本。工程阻塞/难点记录随候选保留。
