# 编辑器维度文件与写入

## 来源与推荐路径

维度字段的语言语义见 [维度规范](../language-design/spec/09-维度.md)。编辑器在基础值之外展示显式覆盖状态、各变体有效值及实际来源文件。

新增覆盖的推荐路径为 `dimensions.<维度名>.dir` 加上主体 CFD 文件的项目相对路径。例如主体为 `data/items.cfd`、维度目录为 `dimensions/language` 时，推荐文件为 `dimensions/language/data/items.cfd`。同一主体文件和维度的记录、字段及变体共用这个推荐文件。

推荐路径由 runtime 的 `dimension_destination` 查询提供，要求维度已配置目录且主体位于项目内。已有覆盖写回其真实来源。覆盖可以分散在多个 CFD 文件中，也可以与基础记录共存；推荐路径只决定新覆盖的默认目的地。

配置的维度目录自动参与 CFD 加载、监听、格式化、LSP 和 Git 比较。不存在的目录作为空目录处理，重复包含的来源去重。推荐文件处于维度目录内，首次创建时无需向 `data` 再添加条目。

## 导航与文件视图

文件树依实际来源组织各维度的目录与文件。纯覆盖文件进入维度导航；含基础记录的混合文件同时保留普通数据入口。基础文件的菜单提供推荐覆盖文件入口，首次填写覆盖时创建实体文件。

`DimensionTarget` 只保存 `dimension` 与实际 `filePath`。维度文件视图使用虚拟路径 `@dimension/<维度名>/<实际文件路径>`，例如 `@dimension/language/dimensions/language/data/items.cfd`。工作区持久化转换其中的实际文件路径；维度名及虚拟入口身份保持稳定。

维度文件表格按“记录与字段”形成行，以变体形成列。每行携带主体文件、记录坐标、字段、基础值，以及各变体的显式状态和来源：

- 分散来源展示实际在该文件中存在覆盖的字段。
- 推荐来源同时展示对应主体的待填字段。
- 同一行保留各变体自己的真实来源，编辑时使用对应来源。

源码及 Git Diff 入口读取实际覆盖文件。Git Diff 分别比较基础记录和维度覆盖，并保留已删除来源的目录位置。

## 状态与事务

结构化查询使用 `DimensionValueState::Missing` 表示没有显式覆盖，`Value` 表示已覆盖。`Value(OptionNone)` 是显式空值，与 `Missing` 不同。编辑器删除覆盖后恢复基础值回退。

写请求携带维度、变体、记录、字段及嵌套路径坐标，并提供预期旧状态。后端将设置和清除分别转换为 `SetDimensionValue`、`ClearDimensionValue`，在统一 mutation 流程中检测并发变化、校验候选源码和发布文件。

新来源进入候选源缓存后随项目统一解析；需要同时改变的 CFD 与项目配置由 staging 事务发布。写入失败保持原会话；发布成功后重新加载数据，返回当前 revision、实际新状态、文件树、维度元数据、诊断与受影响文件。

前端通过 generation controller 接受同一代返回数据。维度写入加入统一撤销和重做历史，避免界面同时展示不同修订的值与文件树。

## 实现入口

- [runtime/query.rs](../../crates/coflow-runtime/src/inspection/query.rs)：推荐路径、覆盖状态与来源查询。
- [session/dimension.rs](../../editors/cfd-editor/core/src/editor/session/dimension.rs)：文件行投影、写请求与返回数据。
- [dimensionNavigation.ts](../../editors/cfd-editor/frontend/src/features/workspace/state/dimensionNavigation.ts)：真实来源树与虚拟路径。
- [dimension_files.rs](../../editors/cfd-editor/core/tests/dimension_files.rs)：推荐来源、分散来源和首次写入场景。
