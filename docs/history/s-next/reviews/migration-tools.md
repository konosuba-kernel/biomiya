# 迁移补测工具独立审查

2026-10-13。审查者 `/root/resume_ids`；未实现本报告审查的工具、fixture 或 P5 平台，只读核查及撰写本报告。曾实现 R-ID，因此此报告不替代 R-ID 生产实现独立验收。

**结论：四个工具的输入构造与核心断言可以承担 G1/G2/G3/G5/G9–G12 的补测职责；未发现由被测编译器生成期待结果、把输入文件误当产物、跳过执行仍报通过等伪证。此为测试设计审查，不是最终候选执行通过。** copy/Blocking 内部 owner API 尚待实现与冻结，不能凭工具存在关闭对应缺口。

## 范围与身份

| 审查文件 | SHA-256 |
| --- | --- |
| `tools/check_q_blocking.py` | `1aba5f7de3057d7cc6414e688385692bb47c0d38b1a8f0f4868c0fe7cc87d1ac` |
| `tools/check_q_limits.py` | `281b0c400a3173b59517be336f56a2521c73af2b10f0d26d4c1c9d366548d28e` |
| `tools/check_q_interfaces.py` | `9ce4e41241a11e6439be78990eb6dc53a65264a4b1f1fda3804d298fff0eaf4e` |
| `tools/check_q_copy.py` | `126ad090bb2c768e94eb36829a7044938c1d2c4a6100cfe672c3f2220519cade` |
| `next/q/tests/blocking.vt` | `6a48bcf56da57c3fdb6c246e614d79adb53ff94043893dee38b0b5323881c7e3` |
| `next/q/tests/copy.vt` | `5d2312f83c7904790a851520c4f8d00db610fbce87e9b3153283211ba3e489b5` |

对照 `reviews/migration-gaps.md`，并实际阅读旧 `tests/b3.rs` 的 Blocking、`tests/b3_frontend.rs` 的深度/资源/精确位置、`src/project.rs` 的清单、`tests/b2_runtime.rs` 的 copy/I/O 断言。没有仅按测试名称认定覆盖。

## 合同核对

| 缺口 | 新断言及判定 |
| --- | --- |
| G1 | Blocking 外部现有 44 个场景：纯递归 Verified，递归 Unknown/Violation，正反调用顺序，死分支和 defer，300 级正反声明顺序，多 require，19 个非法/缺失策略，已有产物及源/策略别名保护。Verified 比较启用/关闭分析的完整 ELF 字节；失败断言无产物。内部 fixture 调用真实 blocking::capture/finish，覆盖未 capture→Unknown、const 被当函数→I_INTERNAL、第二函数第 3 条边达到 65,537→容量拒绝。内部 build 本身单列，不应算第 4 个运行规则。 |
| G2 | 15 项对应旧矩阵：括号 100 层、块 80 层、指针 100 层、类型 40 级；2^32 数组/repeat 长度；空 repeat 仍查类型/名字；可变 static 权限；嵌套 256 字段布局；3000 级字段/索引/else/常量链。明确退出 1，部分要求 actual/limit/file；不接受 signal、panic 或 timeout。长链允许 I_LIMIT/E_TYPE 与原合同一致。 |
| G3 | interfaces 明确第二文件，7 项按人工语义 marker 计算 byte：名字、类型、权限、initializer、前向常量、递归头、递归数组布局。marker 来自输入语法，不从编译器实际输出反推；新的语法位置与旧 cursor 尾位置区别合理且可复核。 |
| G5 | copy 使用真实 input/lex/runtime，三文件包含空文件、长嵌套注释源及 Unicode 路径，期待值为宿主按清单顺序拼接原 bytes；另测全空产物。目标 ELF 实际运行并精确比较输出，而非只检查长度。 |
| G9 | 普通 addr_of 函数返回 42，生成目标实际执行 42，可区分误识为 intrinsic。 |
| G10 | 13 个坏路径、96-byte 合法边界、Unicode 合法路径；重复/缺失/未知指令矩阵；32 source+8 assembly 精确边界和各超一项；16,384-byte 清单及超一字节。UTF-8 路径超限按 bytes。新增输入顺序行为由 copy 负责。 |
| G11 | 语法/名字/类型、三类入口失败、缺入口、链接、非法 UTF-8 在已有 out.elf 下执行；完整目录 byte 快照相等，因此同时验证旧产物和输入未改。名字/类型/语法/入口明确 file 身份，链接按新全局链接阶段诊断。 |
| G12 | read-error、write-error、zero-write、close-error、short+EINTR；每个实际成功 open 都加入 live FD 集合，成功 close 移除且不能关未知 FD，最终集合为空并 opens==closes>0。输入逐字保留；已有输出场景禁止 unlink；写/关闭失败新产物消失并有 unlink trace。 |

## I/O 故障注入边界

注入只修改保存的 `runtime/linux.vt` 副本中的外部 syscall 声明，把它替换为受控 wrapper；真实 read/retry/write/close 实现保留，生产文件末尾检查未变。assembly 副本增加真实 syscall 别名，model 的原诊断 syscall 仍有绑定。每个编译副本、产物 ELF 都记录哈希。

- read-error 在第二个只读 open 后、input::load 读取第一源时返回 EIO；实际已打开 FD 仍由 read_file 关闭。新应用先 load/scan 后创建输出，因此断言“不创建输出”并非漏掉已创建输出的 unlink。旧流式 copy 在输出创建后再次读入，要求 close(4)、close(3) 和 unlink；这部分具体时序已收缩，最终迁移表必须明确，不能宣称固定 FD 序列原样保留。
- close-error 先调用真实 close，再把成功返回值改成 EIO。它证明应用观察关闭失败后移除新产物，并证明此前实际打开的 FD 已关闭；它不证明操作系统返回关闭错误时必然释放 FD。此限制是注入模型的明确边界。
- short/EINTR 会先在一次 read 和一次输出 write 返回 EINTR，再限制真实传输至最多 7 字节。长输入输出逐字相等，实际覆盖重试与部分进度；返回错误数字独立写定，不读取被测编译器结果生成 oracle。

## 执行证据状态

读取现有记录：`q-limits-r2` 15 项已在旧 E3 通过；`q-interfaces-r4` 76 项在 R-ID 候选 `1e4b4c15…0942b0` 通过；`q-blocking-r1-external` 43 项在旧 E3 通过，但当前工具新增 pure-recursion 后为 44 项，旧记录不自动覆盖新增项。内部 Blocking 和 copy 要等最终 owner 候选实际运行。所有最终门禁必须绑定最终候选 SHA，不能把上述旧执行记录移植为新候选通过。

现有 fixture/工具仍可能被后续作者修改。本报告仅绑定上表输入；修改后需复核对应差异和新运行记录。可选证据增强：copy build 的 compiled_inputs 当前在编译后记录；可在编译前先拍快照并在编译后断言一致，更直接排除被测程序改写测试输入。该建议不影响当前源级断言有效性，也不等同已经发现输入被改写。

## 修订二：copy 注入观测与预检记录复核

2026-10-13，仍由 `/root/resume_ids` 独立复核。保留上文首次审查身份；本节仅更新 copy 工具与对应开发预检证据。

- 新 `tools/check_q_copy.py` SHA-256：`679b539b12526e1596029db9f692c391946346f8e2165d1470547091e25d5ce8`。
- `build/q-copy-owner-preflight/verification.json` SHA-256：`c47c48517339f9129acaaeae78153330f9b4d0085e0d71343f18cd6d67ebbad6`。
- 驱动仍是 R-ID `1e4b4c15f3bd613652901364e3ec8a2e5b0e04a5cbe278f3a1948e8f8e0942b0`，不是最终 E3。被编译的 source owner 源为预检记录中的保存快照；此次复核时八个原始输入哈希与工作区相同。

**结论：新增断言有效，预检记录 8/8 可复核；仍不能代替最终 E3 的迁移验收。**

新工具在编译前记录完整生成输入哈希，编译后排除唯一产物 `copy.elf` 再检查完全相等，落实前文提出的输入快照增强。各注入支路通过独立真实 syscall 写 stderr 的 `INJECT ...` 标记，并要求对应标记恰好出现一次；不再仅从成功拼接推定 EINTR 分支确实发生。标记写入直接调用真实 syscall，不会递归进入故障 wrapper。

独立重算 driver、六个 ELF、所有原始快照、六组 compiled_inputs 哈希；逐个验证注入版本在原 extern 声明后的生产运行库文本完整保留。重新解析八项 trace，使用独立 FD 状态字典检查成功 open 不覆盖未关闭 FD、close 必须匹配当前存活 FD、最终全清；并核验实际产物文件内容。结果：

| 场景 | 退出 | 成功 open/close 配对 | 注入标记 |
| --- | ---: | ---: | ---: |
| three-files | 0 | 5 | 0 |
| all-empty | 0 | 4 | 0 |
| short-eintr | 0 | 3 | 2 |
| existing | 12 | 4 | 0 |
| read-error | 1 | 2 | 1 |
| zero-write | 16 | 3 | 1 |
| write-error | 16 | 3 | 1 |
| close-error | 13 | 3 | 1 |

short-eintr 的真实 trace 有超过 100 次成功 7-byte read 和超过 100 次成功 7-byte output write；两个 EINTR 标记各一次。正常三文件与短 I/O 产物均与清单原字节拼接一致，全空产物为零字节；已有产物保持原内容且没有 unlink；失败新产物不存在。上述 read-error/close-error 注入模型边界继续适用。此次是保存记录与文件的独立复核，没有重复执行六轮构建，也没有修改生产或工具。

## 修订三：最终 E3 四项补测正式独立验收

2026-10-13，审查者 `/root/resume_ids`。仅验收未参与实现的四项迁移补测及其证据；不以此自审本人参与过的 R-ID 实现或整部编译器。

**结论 PASS。** 最终 E3 为 `7e1de550da39c31e203c83d0bce7e13dd5deb64923f3c5e4428aad8fbd7d7d20`。独立重算四个账本保存的 driver ELF 与 `build/snext-p4-r2/e3.elf`，完整哈希相同。四工具当前内容、源码输入、保存副本、生成输入、已记录 ELF 身份均匹配；未仅采信主线给出的 passed 状态。

| 证据目录 | 记录数 | verification.json SHA-256 |
| --- | ---: | --- |
| `build/snext-final-blocking` | 48 | `2dce96ee06a07c49822fddf980ee14b5d6671ccfe96e5e6a1d45c29cc108c600` |
| `build/snext-final-limits` | 15 | `dcf9d116c6f5f4b084c7d9f6b259d56f7db65acf370d7b8985316ebd8bce6102` |
| `build/snext-final-interfaces` | 76 | `aa2b53da683097cff0c02eff3eda7e16bdf62607c080d633cccd2283dfb43b8a` |
| `build/snext-final-copy` | 8 | `1f1c27a0ef6182858335cc3de32d81ec969e9ea17eede465800eec11eafeb17a` |

总计 **147 条账本记录**，其中 Blocking 48 条是 **44 个外部场景 + 1 次内部探针构建 + 3 个内部场景**，不得把构建计为额外规则测试。其 `external_only=false`、`internal_contracts_passed=true` 与实际记录相符。

独立脚本另执行了 **25 个目标命令**，全部通过：

- 3 个真实 Blocking 内部探针：缺 capture 为 Unknown；非法 callee 为 I_INTERNAL；跨两个函数累计第 65,537 条 summary edge 为容量拒绝。
- 递归 Verified 图：独立分别构建未启用/启用 Blocking 的完整 ELF，逐字相同。
- 两个 300 级正反声明顺序的调用链均报告 Violation。
- interfaces 的普通 builtin 同名函数、独立类型名字空间、8 个常量目标全部实际运行，分别退出 42、7、以及 0。
- copy 全部 8 项在新目录重新运行；三文件/全空/短 I/O 字节正确，已有输出保留，read/write/zero-write/close 失败均观察到预定路径。逐项重新解析原 trace 和新 trace，FD 状态配平、INJECT 标记次数、清理结果一致。

资源 15 场景逐项核验保存源哈希、退出 1、对应诊断、无新 ELF；接口 76 项逐项核对保存输入，7 项跨文件 marker 与实际 byte 精确相同，已有输出仍为旧字节，清单数量/字节边界与错误分类符合工具已审合同。重复执行是补充区分性复验，没有将全部作者执行描述为本 agent 重新构建。

证据：`build/snext-final-migration-independent/audit.py` SHA-256 `e5cbc1c08670ffa8c6e33a2435fc360c225953fd05b5589c3474765948d59e00`；`audit.json` SHA-256 `19561c2d22739d96798d6701f6f6902d8dccebad6470c86b6a39a21870bf63b9`。**892 项独立审计断言**全部通过；这个数字是哈希、记录、FD 与结果的审计断言数，不是语言样本数。

初稿独立脚本复制内部 probe ELF 后未保留执行位，QEMU 在实际执行前拒绝；失败脚本和记录保留在 `build/snext-final-migration-independent-r1`。仅修审计副本执行位后于新目录重跑成功，没有修改任何生产、fixture 或作者测试工具。

本判定关闭上述四工具覆盖的最终 E3 迁移补测验收。P5 新 E3 系统验收仍独立待执行；8,000 行软预算、完整编译器架构与固定点验收不在本报告授权范围。
