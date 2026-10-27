# 仓库目录与模块组织

## 顶层职责

| 目录 | 内容 |
| --- | --- |
| `src/` | CLI 命令解析、终端输出、执行日志边界、自更新与技能安装适配 |
| `crates/` | 共享语言、模型、检查、运行时、生成器、LSP 与基础设施 |
| `editors/cfd-editor/` | 编辑器共享后端、Tauri 宿主与前端 |
| `editors/vscode-coflow/` | VS Code 扩展及语言资源 |
| `runtimes/csharp/` | C# 加载器、单元测试及目标框架冒烟工程 |
| `tests/` | CLI 和跨模块集成测试；共享工程与语料位于 `fixtures/` |
| `examples/` | 面向用户的完整示例工程 |
| `skills/` | 随产品分发的用户技能与可执行示例 |
| `docs/` | 内部设计契约、工程要求、活动计划与历史资料 |
| `website/` | 公开网站与用户文档 |
| `assets/` | 共享品牌资源与视觉规范 |
| `scripts/` | 仓库维护与性能测量脚本 |
| `packaging/` | 平台安装器和打包资源 |

Rust crate 的职责与依赖约束见 [系统架构](overview.md) 和 [AGENTS.md](../../AGENTS.md)。CLI 保持二进制宿主；非 CLI 宿主通过共享 crate 接入项目能力。

`coflow-observability` 负责宿主日志初始化、容量控制与诊断快照。CLI 的 `command_logging.rs` 只适配命令执行边界；编辑器的原始错误详情由前端 `features/diagnostics/` 管理。C# XID 判定数据的生成入口归 `coflow-language/examples/`，产物及加载逻辑归 C# Loader。

## 运行时内部

`coflow-runtime` 是共享项目边界，内部按项目流程组织模块：

| 模块 | 职责 |
| --- | --- |
| `project/` | 项目配置、路径规则、输入发现与项目初始化 |
| `loading/` | schema 编译、CFD 数据加载、来源解析与加载诊断 |
| `sessions/` | 项目代次构建、会话刷新与读写访问入口 |
| `inspection/` | 记录视图、查询、搜索、schema 检视、统计与差异 |
| `checks/` | 项目级检查规划、影响范围与增量检查状态 |
| `mutation/`、`writes/` | 修改准备、候选验证、来源写入规划与暂存 |
| `publication/` | 源码和生成产物的发布及持久化结果 |
| `automation/` | 结构化查询、修改请求和事务协议 |
| `commands/` | 共享项目命令编排 |
| `api/` | 宿主共用的协议类型 |

固定 CFD 读写实现位于私有 `cfd_loader/`；来源索引和维度等跨流程模块保持独立。内部模块通过 crate 内路径协作，对外类型与入口由 `lib.rs` 导出。

## 编辑器前端

`frontend/src/app/` 负责应用装配、查询客户端配置和视图模块加载。业务能力位于 `features/`：

| 功能目录 | 职责 |
| --- | --- |
| `workspace/` | 项目代次、导航、标签页、设置、选择与撤销历史 |
| `record-editing/` | 记录创建、字段编辑、批量修改、引用展示与值投影 |
| `table/` | 表格和维度表格、单元格渲染、选择与预览窗口 |
| `graph/` | 图谱视图、节点渲染、端口几何、布局与 worker 适配 |
| `source-editor/` | 源码编辑、保存、搜索与语言服务适配 |
| `git-diff/` | 项目比较、差异投影与行对齐 |
| `diagnostics/` | 诊断展示、错误边界与问题记录 |
| `plugins/` | 插件运行、宿主接入与内置贡献 |
| `search/` | 项目搜索入口 |

功能自己的组件、状态、hooks 和测试放在同一功能目录。`shared/ui/` 保存通用基础组件，`shared/lib/` 和 `shared/motion/` 保存跨功能工具；依赖具体功能的上下文归该功能维护。共享代码不反向依赖业务功能。

`api/` 负责宿主调用，`wire/` 负责协议值与路径转换，`bindings/` 保存后端生成类型。浏览器预览数据和演示组件位于 `test-support/`，浏览器集成测试与页面入口位于 `frontend/tests/browser/`。图谱纯布局与浏览器 worker 适配分别维护；视图模块的预加载与延迟加载共用 `app/viewModules.tsx`。

记录标题、位标记算法和搜索高亮各自独立；图节点渲染与图谱交互调度分离；表格单元格保持稳定组件身份，列快照刷新时保留编辑草稿。

## 测试资源

crate 自有测试放在对应 crate，跨模块测试和共享语料放在根 `tests/`。`tests/fixtures/editor-project/` 提供完整编辑器工作流工程，`tests/fixtures/editor-inheritance/` 提供继承与引用回归工程。解析和运行时一致性语料分别位于 `cfd-parser-parity/` 与 `cfd-runtime-parity/`。

## 内部文档

当前设计契约按语言、架构、运行时、编辑器和代码生成职责组织，由 [文档索引](../README.md) 导航。`development/` 保存工程规范、测量协议和优化候选。运行时增量与前端投影契约分别位于 `runtime-design/` 和 `editor-design/`。`archive/` 保存带代码版本和环境的性能测量报告及原始证据。

文件移动时同步修改模块声明、导入、测试入口、脚本引用和文档链接。生成绑定仍通过现有导出流程维护。开发与发布检查以 [AGENTS.md](../../AGENTS.md) 为准。
