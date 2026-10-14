# P3 独立验收预检与组合探针

日期：2026-10-12。实际 agent：`/root/next_review`。状态：**候选尚未冻结，未编译/执行下列新探针，不给出 P3 通过结论。** 没有修改生产源码或现有测试输入。

## 已核对覆盖与阶段边界

当前 p3/cases.json 为 126 个唯一身份：P2 的 98 项逐项原样保留，另加 28 项枚举样本。新增覆盖载荷和无载荷构造、unit 与无载荷区分、布局、zero、参数个数/类型、穷尽性/重复分支/通配拒绝、名义类型、递归、导入、公开载荷、静态及重复重定位。既有 P2 测试没有被删改。

trials/typed.vt 与 tagged.vt 的对照保留了相同构建算法、范围/活跃/世代检查，能用于同功能成本对照。measure.py 仅测源码并预测布局；README 正确声明尚未执行。其 7 个用例必须用正式候选执行，记录 ELF/源/工具身份、准确错误码和输出，而不是将预测的 sizeof 当成实测。

已向 root 反馈并得到确认：

- check_next 排除 trials/system；P3 将另有绑定候选和全部依赖的试点执行账本。三类具体 I/O 结果需要实际 Linux 程序验证，不能只计类型声明行数。
- P 本轮只是私有受检表和 driver begin/end，主线完整 nominal ID、世代与真实 AST/IR enum 迁入 Q 后单独验收。toy trial 的世代通过不能替代主线旧 ID 隔离。
- root 将在 DAG 明确 P3 表示支持/试点与后续 Q 所有权门槛的依赖；Integration 之前不能宣称原 P3 全部最终合同完成。最终报告将分别判定，不默默把原计划门槛删掉。

## 待冻结后执行的区分性探针

以下是独立预期，未参考未冻结候选的输出反推。每例应 check/build；拒绝例检查错误码与不发布 ELF，成功例实际运行。固定使用 qemu 1 MiB guest 栈、空 PATH，保存样本全文及候选哈希。若生产已有等价新增测试，正式独立复验仍选少量组合而非机械重跑全部。

### 1. 聚合载荷的普通参数、return 与 match 快照

期望退出 42。后续调用实参修改原枚举；return 前的清理把原枚举清空；match 分支内改写原匹配对象。三个地方读取的旧聚合载荷都必须保持。

```text
module sample;
struct Pair { x:u32, y:u32 }
enum E { Empty, Pair(Pair) }
unsafe fn replace(p:*E)->u32 {
    unsafe { *p=E::Pair(make Pair{x:90,y:91}); }
    return 0;
}
unsafe fn clear(p:*E)->() { unsafe{*p=E::Empty;}return(); }
fn sum(e:E,ignored:u32)->u32 {
    match e {
        E::Empty=>{return 0;},
        E::Pair(p)=>{return p.x+p.y;},
    }
}
fn saved()->E {
    var e:E=E::Pair(make Pair{x:20,y:22});
    unsafe{defer clear(core::addr_of_mut(e));return e;}
}
fn run()->u32 {
    var e:E=E::Pair(make Pair{x:7,y:35});
    unsafe {
        if sum(e,replace(core::addr_of_mut(e)))!=42_u32{return 1;}
    }
    if sum(saved(),0)!=42_u32{return 2;}
    e=E::Pair(make Pair{x:19,y:23});
    match e {
        E::Empty=>{return 3;},
        E::Pair(p)=>{
            unsafe{let ignored=replace(core::addr_of_mut(e));}
            return p.x+p.y;
        },
    }
}
```

### 2. 非活动私有载荷不阻止 Empty zero，活动载荷不泄露私有字段

两个文件的成功版本应返回 42。E 的第一项无载荷，因此其 zero 有效；H 的私有字段不属于活动载荷，不能递归拒绝 E 的 Empty zero。H 通过所属模块的工厂创建，外部可以作为完整值构造/匹配 E::Some。

values.vt：

```text
module values;
pub struct H { x:u32 }
pub enum E { Empty, Some(H) }
pub fn create()->H { return make H{x:42}; }
pub fn read(h:H)->u32 { return h.x; }
```

main.vt：

```text
module sample;
import values as v;
static Z:[2]v::E=zero;
fn run()->u32 {
    match Z[1] {
        v::E::Empty=>{},
        v::E::Some(h)=>{return 1;},
    }
    let e=v::E::Some(v::create());
    match e {
        v::E::Empty=>{return 2;},
        v::E::Some(h)=>{return v::read(h);},
    }
}
```

对照拒绝例只将最后的 `v::read(h)` 换成 `h.x`，期望 E_NAME；不能因为匹配到了载荷就获得载荷类型私有字段权限。另将 v::create() 换成 `make v::H{x:42}` 也应拒绝，枚举构造不是字段可见性的后门。

### 3. 嵌套变体重复初始化与重定位

期望退出 42。两个 enum payload 层包住指针及字符串，外层数组重复两次。重复 serializer 必须复制正确字节并逐一复制重定位，不能只让第一个元素可用。

```text
module sample;
static TARGET:u32=40;
struct Ref { p:*const u32, text:[]const u8 }
enum Inner { Empty, Ref(Ref) }
enum Outer { Empty, Inner(Inner) }
static TABLE:[2]Outer=[Outer::Inner(Inner::Ref(make Ref{
    p:core::addr_of(TARGET),text:"ok"
}));2];
fn run()->u32 {
    match TABLE[1] {
        Outer::Empty=>{return 1;},
        Outer::Inner(inner)=>{
            match inner {
                Inner::Empty=>{return 2;},
                Inner::Ref(r)=>{
                    if r.text[1]!=107_u8{return 3;}
                    unsafe{return *r.p+(r.text.len as u32);}
                },
            }
        },
    }
}
```

对照拒绝例把整个 initializer 改为长度 0 的重复，并把活动载荷内的 u8 值设成 256；即使没有输出元素，build InitData 仍必须检查非法载荷。现有 enum_zero_repeat_invalid 已覆盖简单版本，不必机械重复。

### 4. 匹配绑定的词法边界与清理

补测 match 每个分支中的 defer 在分支结束/return 时恰好执行一次，且另一分支不会继承清理列表；匹配表达式有可观察副作用时只求值一次。对照拒绝例在 match 结束后使用载荷绑定名，期望 E_NAME。已有 enum_missing_arm/duplicate_arm/wildcard 可沿用，不用重复制作同构例。

需要确认旧的整数/布尔 match 没因新增 enum 分支而丢掉默认分支和类型检查；原 98 样本加一项混合函数可检查不同 match 种类在函数 arena 复用后的隔离。

### 5. 名义 ID 与真实生命周期

trials 的正确程序已检查 end 后和新 begin 后旧 ID 不活跃。专项 stale_id 应实际调用 owner 的读取/构造入口，避免只测一个返回 false 的自述函数。还需至少覆盖：

- begin → 分配 ID → end → begin → 分配相同索引 → 旧 ID 读取失败。
- 新函数的合法 ID 可用；Header/静态初始化的持久 ID 不被函数重置错误清除。
- 正在构建时不能把未封口函数交后端；封口后语义表不能继续被 public setter 写入。
- 容量满时是可解释的资源失败；零 ID 和超界 ID 拒绝。

前三项必须最终对 Q 生产 owner 验证。P 当前裸 u32 接口即使检查 active 和 count，旧 ID 数值在下次函数重新分配后仍可能有效，不能据此宣称世代隔离。若仅提供公共 setter 加 index 检查，也不能自动宣称“已完成表只读”。

## 成本与归档的验收要求

P2 r5 已有 8,102 生产行。P3 需把新 enum 解析/布局/检查/后端、owner 接口、三类 I/O、trials 分别记账，并报告净变化。Q 的真正 AST/IR 替换与 ID 包装另有实际成本，预测不能当实测。测试、工具、冻结 P/B 也需分开列出，不以移目录掩盖运行所需代码。

trial `private_id` 当前只有 reject=true；正式执行时建议明确 E_NAME，并检查不是找不到模块或语法错误产生的任意非零退出。所有试点 hash 必须包含 `.build`、`.vt`、平台 assembly 和执行工具，且构建/运行前后保持一致。候选变动后这些证据需要复核。
