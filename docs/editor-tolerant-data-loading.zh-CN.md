# CFD 编辑器容错数据加载设计

## 1. 目标

CFD 编辑器只对能够由既有 `CfdDataModel` 完整、无歧义表达的数据提供结构化编辑。无法表达的数据保持原始源码不变，通过完整诊断进入源码编辑器修复。局部数据错误不得阻止无关文件和记录加载。

本设计不引入通用 `EditableValue`，不允许前端按错误码推断处理方式，也不保留新旧协议兼容分支。

## 2. 加载边界

项目加载依次执行：

1. 读取项目配置并编译 schema。
2. 独立加载每个数据源，累积全部诊断。
3. 验证每条记录，将其明确归入 accepted 或 rejected。
4. 只为 accepted 记录建立表、记录和引用索引。
5. 只对 accepted 模型执行 check。
6. 合并项目、schema、数据源、数据模型、引用和 check 诊断。
7. 返回包含部分模型和完整诊断的 `ProjectBootstrap`。

普通 CFD 数据错误不导致项目会话创建失败。只有项目配置无法建立、schema 无法编译、必要文件系统操作失败或运行时内部不变量损坏时，加载才整体失败。

## 3. 错误隔离

| 问题 | 模型行为 | 编辑入口 |
| --- | --- | --- |
| 缺失必填字段 | 接纳记录并保留缺失状态 | 表格字段或记录视图 |
| 引用目标丢失 | 接纳记录并保留非法引用状态 | 表格字段或记录视图 |
| 引用类型不匹配 | 接纳记录并保留非法引用状态 | 表格字段或记录视图 |
| check 失败 | 接纳记录 | 最具体的字段或记录视图 |
| 多余字段 | 拒绝整条记录 | 源码范围 |
| 类型错误 | 拒绝整条记录 | 源码范围 |
| 非法枚举成员 | 接纳记录并保留非法枚举空态 | 表格字段或记录视图 |
| 非法 flag 枚举表达式或类型 | 拒绝整条记录 | 源码范围 |
| 重复字典 key | 拒绝整条记录 | 源码范围 |
| 未知、抽象、Host 或不兼容类型 | 拒绝整条记录 | 源码记录范围 |
| 非法记录 key | 拒绝整条记录 | 源码记录范围 |
| 重复记录 key | 冲突集合全部拒绝 | 各自源码记录范围 |
| 局部 CFD 语法错误 | 拒绝无法恢复的源码单元 | 源码错误范围 |
| 整个文件无法解析 | 拒绝该文件的数据 | 源码文件 |
| 数据源读取失败 | 继续加载其他数据源 | 文件或项目配置 |

编辑器为无法由默认值补全的必填字段生成 `missing = true` 单元格：其 `value` 仅是由 schema 类型生成的编辑种子；无法自然生成种子的必填引用使用 `OptionNone` 作为编辑器空态。schema 默认值和省略的 `Option<T>` 由核心模型正常物化，均不是缺失错误。禁止把其他类型错误伪装成缺失值，禁止从重复 key 中选择任意一条记录作为有效记录。被拒绝内容不得进入运行时索引、check、代码生成或发布模型。

表格、记录视图和检查器必须将 `missing = true` 渲染为统一的 `Missing` 状态和修复按钮，不得展示编辑种子的普通值控件。点击修复后一次性提交编辑种子；必填引用选择目标类型的第一条可用记录。没有合法种子或引用目标时禁用修复按钮并明确提示，不写入伪造值。

## 4. 诊断导航契约

每条传给编辑器的诊断必须由核心库给出明确目标：

```rust
enum DiagnosticTarget {
    TableField {
        file_path: String,
        coordinate: RecordCoordinate,
        field_path: String,
    },
    Record {
        file_path: String,
        coordinate: RecordCoordinate,
    },
    Source {
        file_path: String,
        range: Option<TextRange>,
    },
    ProjectSource {
        file_path: String,
        range: Option<TextRange>,
    },
    None,
}
```

parser 和 loader 负责源码目标；data model builder 和引用解析器负责记录是否可结构化；checker 负责逻辑字段目标。前端只执行目标，不检查诊断 code、stage 或可选位置字段来猜测视图。

底部诊断面板始终显示当前 revision 的完整诊断集合。reload、watcher 和 mutation 使用新 bootstrap 原子替换诊断，不追加旧 revision。诊断目标不可用时必须在核心库产生 `None`，前端不得尝试跳转后再兜底。

## 5. 写入约束

结构化 mutation 只能操作 accepted 模型中存在的记录，以及 schema 已声明的字段。字段在源码中缺失时，writer 以明确的记录坐标新增该字段；rejected 记录仍只能通过源码编辑器修复。writer 必须执行定位明确的局部修改，不能重写未加载的记录或通过序列化整个文件覆盖未知源码。

mutation 发布成功后重新加载完整项目并返回完整 `ProjectBootstrap`：

- `write_ok` 只表示文件原子发布成功。
- `check_ok` 表示完整诊断中不存在 error。
- 新 revision 可以包含数据错误。
- mutation 无法定位、写入、重新建立项目基础结构或发布时才回滚。

## 6. 严格消费者

编辑会话使用 partial build。CLI check、代码生成、运行时加载和发布继续使用 strict build；只要存在任何 error，strict build 必须失败。partial model 不能通过公共 API 被误当成可发布模型。

## 7. 迁移要求

迁移必须一次完成并删除：

- 旧 `build_editable()` 命名和 mutation 层维护的第二份可编辑错误码白名单。
- `Option<RecordDraft>` 造成的无原因静默丢弃。
- 重复记录选择第一条的行为。
- 前端 `isJumpable`、`onJumpToRecord`、`onJumpToField` 和诊断跳转推断。
- 旧 `FlatDiagnostic` 导航字段及新旧 DTO 转换。
- 普通数据错误触发空模型的全局 fallback。
- 兼容别名、deprecated 类型和双写协议。

最终只保留 `build_partial()` 与严格的 `build()` 两个入口，共享同一验证实现。

## 8. 验收

必须覆盖跨文件和同文件错误隔离、一次返回全部诊断、重复 key 全部拒绝、rejected 数据不进入引用/check/codegen、源码修复后进入结构化视图、结构化修改不改变 rejected 源码、各类诊断跳转、revision 替换以及 strict build 拒绝任何 error。缺失必填字段、非法引用和非法枚举成员必须保留记录坐标与字段路径，可从结构化单元格直接修复；省略的 schema 默认字段和 `Option<T>` 不得标记为缺失。

正常开发门禁为：

```powershell
cargo check --workspace
cargo test --workspace
```

编辑器协议和界面变化还必须通过 frontend test 与 production build。
