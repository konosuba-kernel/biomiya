# R-r1：路径与保存失败合同修复

2026-10-14；作者 `/root/fix_io_failures`；基线 `2c7061cbed13959697369879e5717a8ad0c9fb21`。
候选已冻结，待 root 独立验收；不包含自举完成声明。

## 生产变更

- `runtime/linux.vt`：单一 `copy_path` 供 open/remove/save 使用，拒绝空、超长、内嵌 NUL。open/remove 的原有错误码保持不变。
- 同模块新增具体 `save_new`/`SaveResult`，拥有文件描述符的整个生命周期：O_EXCL create（0600）→完整 write→chmod（0755）→显式 close。write 失败不执行 chmod；create 失败不 close/unlink，已有输出不受影响。
- 复用私有 open/write/close/unlink helper，内部保存真实 Linux errno；原公共 I/O 接口仍适配既有固定错误码。没有全库错误 API 迁移，也没有通用事务框架。
- `SaveError` 保存主 phase/errno、close_errno、cleanup_errno。保留最早失败，close 仍恰好尝试一次；Linux close 不能安全重试。失败后尝试 unlink；如果文件系统拒绝删除，无法保证文件消失。
- `input.vt` 拥有路径和终端诊断。保存失败在现有 `E_IO file=0 byte=0` 前追加 path、phase、errno；清理失败显式打印 residual 路径。stderr 本身失败时不替换原始保存错误。
- `save_new` 标明调用者在同步阻塞期间保持 path/data 可读并独占输出路径；不承诺并发 pathname 替换安全、断电事务或持久化。旧裸切片 API 的整体收紧仍留给 K0。

示例（写失败加清理失败）：

```
save path=result.elf phase=write errno=5 close_errno=0 cleanup_errno=13 residual=result.elf E_IO file=0 byte=0
```

零进展/不可能写入数量映射为 EIO(5)，这是本地协议异常映射，已在 helper 注释；其余保存 syscall 错误保留真实 errno。

## 反例与验证

新增 `tools/check_q_io_failures.py` 使用显式 `require`，没有依赖 `assert` 的验收。冻结 E3 编译生产模块副本，测试 shim 只在真实 `input::load` 后激活，按 syscall 边界注入故障；argv 存储修改和 stderr 事件标记仅存在于测试 shim。源码、shim、ELF、命令和实际输出保存在 ignored build 记录。

12 例在普通 Python 与 `python -O` 均通过：正常保存、已有输出、短写、部分写后失败、零进展、chmod 失败、close 失败、write+unlink、write+close+unlink、chmod+unlink、close+unlink、NUL 开/删拒绝。

事件序列验证失败写不 chmod、只 close 一次；真实磁盘内容验证成功输出完整且 0755、已有文件字节/0640 不变、单故障输出删除、双故障的 7 字节部分文件为 0600。close+unlink 双故障可能保留完整 32 字节 0755 文件，诊断明确说明失败与残留；该场景不把完整写入误称为部分写。

对真实修改模块的既有消费者验证：`check_q_input.py` 24 例、`check_q_copy.py` 8 例通过。曾并行执行旧 E3 的 interfaces 76 例并通过，但它检查旧编译器行为，因此不计作本候选的语义验证；集成 I 应使用新固定点 E3 重跑。

| 命令（均在 biomiya 根目录） | 记录 |
|---|---|
| `python core-s/tools/check_q_io_failures.py --compiler core-s/build/snext-p4-r2/e3.elf --record-dir core-s/build/behavior-fixes-r-io-r1` | 普通12例 |
| `python -O core-s/tools/check_q_io_failures.py --compiler core-s/build/snext-p4-r2/e3.elf --record-dir core-s/build/behavior-fixes-R-io-optimized-r1` | 优化模式12例 |
| `python core-s/tools/check_q_input.py --compiler core-s/build/snext-p4-r2/e3.elf --record-dir core-s/build/behavior-fixes-R-input-r1` | 既有24例 |
| `python core-s/tools/check_q_copy.py --compiler core-s/build/snext-p4-r2/e3.elf --record-dir core-s/build/behavior-fixes-R-copy-r1` | 既有8例 |

以上不是原来旧编译器直接通过新语义测试：E3 先编译本候选的实际生产模块，再由 QEMU 执行该探针。尚未完成候选编译器自身的固定点验证，由 I 负责。

## 成本与边界

生产文件增删：input +54/-33；runtime/linux +114/-25，合计净增 **110 行**（两个文件当前合计644物理行）。其中包括具体错误记录、原 errno helper、保存生命周期和路径诊断；没有新增生产文件。新增测试工具171行、VT探针24行、S0故障注入153行，共348行。函数 `save_new` 34行，完整保存路径集中一处。

未修改冻结 B/P/Rust、旧审计探针、旧 release 和旧验收证据；未修改其他编译器模块或提交。候选源码、精简验证记录、报告 hash 见 `R-r1-candidate.json`，完整生成物只留 build 下。
