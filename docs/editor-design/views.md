# 编辑器视图与设置

## 视图结构

每个业务文件和实际记录类型分别维护视图。内置记录、表格和源码视图使用保留的 `__` ID；自定义表格和图谱视图具有独立 ID、名称、kind 与字段配置。自定义 ID 不能使用保留前缀，名称最多 80 字符。单例使用内置视图，其他类型按图谱能力及已保存配置提供自定义视图。

表格视图的 columns 保存列顺序，图谱视图的 fields 保存展示字段，relations 保存可见关系字段；group_filter 限定记录分组。业务数据、单例、维度与 Git Diff 默认使用表格视图，记录视图提供详情浏览，源码视图提供原文件编辑。

视图标签可以排序。保存时清洗视图 ID、名称、字段列表、列宽与分组配置，渲染时按当前字段模型应用投影；当前视图不可用时选择适用的内置视图。插件视图使用插件贡献 ID 并由前端插件宿主管理生命周期。

## 共享与本机设置

项目共享设置位于 `editor-setting/editor.json`，version 为 1，只保存团队同步的数据：

- graph_positions：视图或类型的节点坐标。
- short_name_fields：记录类型的缩略名字段。
- views：业务文件、类型与自定义视图结构。
- record_groups：记录分组及成员。

本机设置同样使用 version 1，位于用户数据目录下 `coflow/cfd-editor/projects/<规范化项目路径摘要>/editor.json`：

- graph_compact_modes：图谱紧凑显示。
- view_order：视图标签顺序。
- default_table_column_widths：默认表格列宽。
- view_column_widths：自定义视图列宽。
- workspace：打开标签、当前标签、记录坐标和维度目标。

项目路径摘要使用规范化绝对路径的 SHA-256，隔离不同项目。共享文件使用项目相对路径，本机字段在读写时转换路径。列宽只保存有限值并满足 48 的最小宽度。

读取只组合当前共享格式和本机格式，版本不匹配时返回诊断。共享字段变化才更新项目文件，个人展示变化只更新用户目录。设置写入采用同目录临时文件和原子替换。

## 工作区恢复与维度

工作区 wire 使用 file_path、type_name、view_id、view_kind、dimension_target 和 active_tab_id。恢复时验证文件、类型、记录坐标、视图与维度目标是否仍存在，再生成内部标签与路由。

维度标签使用虚拟文件入口 `@dimension/<name>/<filePath>`；dimension_target 保存维度名与实际来源文件路径。持久化时转换实际文件路径，按两者恢复标签身份。文件行与覆盖来源的组织见 [维度文件与写入](dimensions.md)。

## Git Diff

Git Diff 与基础模式共用文件树、表格、记录视图及字段树。文件树按文件和类型导航，维度覆盖按维度及实际来源文件组织，HEAD 独有的文件保留对应目录层级。基础记录与各维度覆盖分别比较，混合文件可同时提供基础数据和维度入口。

两端快照分别保留所属 schema 的字段 label、描述与嵌套字段展示信息。差异定位到对象字段、数组索引和字典键；变化子字段的祖先默认展开，表格前后版本和记录左右窗格共享展开状态。记录窗格按相同字段路径对齐行高与空白，并同步纵向滚动。

## 撤销与发布

共享的视图结构和图布局变更由 mutation history 串行维护；本机列宽、标签顺序与工作区状态按各自设置命令保存。后端完成写入后返回当前设置或数据版本，前端仅接纳当前会话与 revision 的结果。

## 模块职责

core 的 settings/model 维护磁盘模型和路径转换，settings/io 负责原子读写与版本校验，settings/sanitize 负责配置清洗。SessionStore 的设置命令桥接 runtime 查询与持久化，Tauri 只适配命令和事件。

前端 workspaceTabs 管理标签和 wire 转换，views 管理内置与自定义视图，generation controller 接纳后端数据，mutation history 串行执行撤销和重做。wire.ts 为各领域 wire 工具的统一公共入口；图布局纯计算与 Worker 客户端各自独立。
