# P4 验收工具使用约定

`tools/selfhost_next.py` 提前准备引导链验收。**目前尚未用实际 P3/Q 运行；工具逻辑测试不构成 P4 完成。** 工具只产生机械证据，不自动接受语言实现或架构迁移。

## 调用

从 `core-s/` 执行；各参数均为实际路径，结果目录必须不存在：

```sh
python3 tools/selfhost_next.py \
  --p3-record build/已接受的P3验证目录 \
  --seed build/b31-refactored.elf \
  --q-manifest next/q/compiler.build \
  --record-dir build/p4-全新记录目录
```

四个参数都是必填。工具不修改 P、Q、种子、P3 记录或其他工具。Q 清单必须位于 core-s 内，以 core-s 为基准列出全部 source/assembly；不接受绝对或越界源路径。Q 应使用独立文件路径，不能先覆盖 P3 的 next/*.vt，再把此前 P3 结论用于已改变源码。

Q 入口模块必须只有一个 `text("S-next...")` 统计前缀调用，且这个字符串字面量在该文件唯一。工具通过 entry 的模块名定位文件，不硬编码 Q 目录或 main.vt 路径。若未来驱动改变输出形式，应明确更新此合同及扰动检查，不能悄悄跳过行为实验。

## 执行门槛

1. 上游记录 stage 必须为 P3，failed 为空，全部 case 完成，passed 数与 case 数一致且名称不重复，11 项既有目标检查存在。此处检查的是记录完整性；**“独立审查已接受”仍是提供 --p3-record 的操作者责任**。
2. candidate-source.tar 必须与记录中的 source_sha256 / verification_inputs 完全一致；当前 P3 输入也必须匹配。封存 tar 只接受无越界路径的普通文件，拒绝链接、重复成员和超限内容。
3. 种子 B 的 ELF hash、P3 记录、next/seed.json 三方一致；冻结基线 source identity、当前基线输入以及 seed-source.tar 一致。
4. 在独立临时树重新运行 B(P)=E0，E0 必须等于 P3 已验证候选 hash。后续每代使用一个新临时树，只放声明输入、归一化清单和该代 driver.elf。
5. 运行 E0(Q)=E1、E1(Q)=E2、E2(Q)=E3，完整 ELF 字节必须 E2==E3。不会用固定的 E0 或宿主编译器代替后续代。
6. E3 check 一个小样本，确认原统计前缀；只替换 Q 主入口统计字面量为唯一标记，E3 编译 Q' 得到另一编译器，再运行该产物 check 同一样本，必须看到新标记、看不到原前缀。仅改变 ELF hash 不算通过。
7. 返回前再次核验 P3、Q、冻结基线、B 和工具源码，拒绝运行期间版本变化导致的混合证据。

每次目标执行使用绝对路径 QEMU、空环境中的 PATH、1 MiB guest 栈，stdin 为关闭的输入源。`-strace -D <日志路径>` 将 guest syscall 与程序 stderr 分离，保证统计和扰动前缀可实际读取。guest 每个 syscall 都检查：只允许当前直接运行支持需要的文件读取/写入/关闭/权限/退出操作；open 只能读声明源/清单或排他创建这一代输出。输入不能写、输出不能读；不允许 guest 打开自身 ELF、/proc、自带编译器、宿主服务或未声明路径，不允许 execve、fork/clone 或网络调用。QEMU 在宿主端初始装载 driver.elf 不属于 guest 读取自身 ELF。

允许的 syscall 集合是此版本运行支持的具体合同。将来增加合法 runtime syscall 应更新验证器并重新审查，不能关闭整个白名单。该验证不是恶意编译器正确性的形式化证明：它证明观测到的文件/执行行为受限，配合源码身份与可观察扰动提供证据。

## 结果内容与资源含义

- `b.elf`、`e0.elf` 至 `e3.elf`、`perturbed.elf` 保存可追溯执行产物。
- `p3-verification.json`、`seed.json`、`p3-candidate-source.tar`、`seed-source.tar` 保存上游输入身份。
- `q-source.tar` 保存原始 Q 清单及所有声明输入；`perturbed-source.tar` 保存唯一字面量变化后的同一组输入；`tool-source.tar` 保存本工具和导入的 memory() 实现。tar 时间戳固定，文件内容逐项 hash。
- `verification.json` 记录每代 driver hash、完整输入 hash、归一化清单 hash、命令、空 PATH、输出 hash、耗时、统计和访问摘要。
- `logs/` 保存每次目标的 strace、stdout 和 stderr；失败及超时也保留已有记录，不覆盖已有结果目录。

每个生成 ELF 都执行冻结 `verify.memory()` 的 64 MiB RW / 映像地址跨度门槛，并验证 ELF64/RISC-V/装载段基本边界。沿用最大单函数 frame 256 KiB 门槛和 1 MiB 实际 guest 栈；记录 max_ast/max_ir 等编译器实际报告的计数。静态段和单函数 frame **不是**宿主 RSS，也不是调用链峰值的静态证明。工具未伪造 RSS 数据。

成功仅设置 `mechanical_checks_passed: true`、`e2_equals_e3: true` 及扰动观察结果；`independently_accepted` 始终为 false，`language_cases_required` 为 true。正式接受仍需外部独立语言正反例、机器/ABI检查、架构审查与对应版本参考。P5 还需使用同一 E3 hash 另行执行系统切片。

## 目前完成的工具自测

- 参数帮助正常；把真实 P2 记录冒充 P3 会在预检拒绝，并写出 mechanical_checks_passed=false 的失败记录。
- 冻结 B 对现有环境探针的真实 QEMU `-strace -D` build/check 日志通过解析；人工加入 execve、读取 driver.elf、读取未声明继承 fd 的日志均拒绝。
- 用显式标记为不可执行的假 ELF、内存中的 subprocess 替身和假 P3/Q 文件，走通 7 步状态机及归档/记录；另外确认 E0 不匹配、E2/E3 不匹配、扰动无可观察变化都记录失败。这些替身没有运行新编译器，输出只在 `/tmp/EXPLICIT-p4-tool-double-*`，带 `EXPLICIT_TEST_DOUBLE` / `NOT_P4_EVIDENCE` 标记，不能归入正式 P4 证据。

当前真实 Q 不存在，因此没有声称新语言固定点或扰动实验已通过。正式调用必须使用新建结果目录，不复用这些工具测试替身。
