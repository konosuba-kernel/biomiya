# 编译器运行环境边界

当前 S-next 核心通过 `runtime::host` 调用环境操作。目标 profile 与编译器运行环境分开：
Linux 上的编译器可以生成 suba ELF，suba 中的编译器也使用同一个前端、分析器与后端。

| 环境 | 实现 | 支持 |
|---|---|---|
| RV64 Linux | `compiler/runtime/host_linux.vt` 与 `linux.vt` | 当前自举与回归环境 |
| suba | `compiler/runtime/host_suba.vt` 与相邻 suba 的 libsuba | K7 原生应用编译接入；固定点另属 K8 |

两种 host 实现不可同时链接。suba 的 `tools/build_users.py` 选择当前 `compiler/compiler.build`
中的核心文件，替换 host 和系统调用桥，并保存实际输入身份。此过程不复制一套编译器核心。

host 提供 argv、整文件读取、独占新建保存、诊断和退出。读取返回具体的
`ReadResult::Value / TooLarge / Failed`；容量不足不是跨环境的某个神秘错误数字。
Linux 底层的历史错误17仅在 Linux host 内转换，输入与 Blocking 分析不依赖它。

suba 保存输出使用 Transient、新建独占、完整写入、sync+seal、关闭。错误保存实际阶段、
主错误、关闭与清理错误。编译器仅生成 Job/out 文件，不发布 `/work`：构建工具在检查退出和
ELF 后承担原子发布，取消时内核负责回收。诊断流关闭不覆盖编译器原来的失败原因。

K7 工程记录与真实系统验收位于 `suba/docs/workflows/through-k7/`。能够生成 suba-user ELF
不等于在 suba 中运行过编译器；原生应用闭环也不等于编译器 C0→C3 固定点已经完成。
