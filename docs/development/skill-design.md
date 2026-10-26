# Coflow 用户技能设计

技能是随 CLI 分发的用户操作指引。网站按功能提供完整参考，技能按任务提供读取、修改、完成判断和错误处理。二者独立组织内容，命令能力与语言语义保持一致。

## 边界和结构

- coflow-schema：新项目、类型设计、默认值、规则、引用、多态、维度建模和关联源码变更。
- coflow-workflow：已有结构下的查询、记录和批量修改、覆盖维护、恢复、代码生成、应用接入和插件。

SKILL.md 保存简短主流程和任务路由。references 每篇回答一个任务或一个必要语义问题，仅读取当前任务需要的资料；examples 保存可运行的项目与请求。两个技能可独立安装，各自保留完成本职任务所需的核心概念和项目配置说明。

任务资料说明适用条件、必要输入、操作方式、完成条件及相关异常；简单概念资料无需套用固定流程。具体参数和 JSON Schema 通过已安装 CLI 的 describe 查询，技能保留常用命令、输入实例和跨命令约定。

本地链接只指向同一技能内的资源，常规任务不依赖联网读取网站。依赖安装等本身需要外部资源的任务给出来源与版本匹配方式。

## 实现依据

| 内容 | 核对位置 |
| --- | --- |
| CLI 参数、自描述及请求入口 | src/cli.rs、crates/coflow-runtime/src/automation/ |
| JSON 值、路径与变更集 | automation/model_value.rs、operations.rs、transaction.rs |
| 初始化和配置路径 | crates/coflow-runtime/src/project/ |
| 类型、默认值与检查规则 | docs/language-design/spec/、coflow-language、coflow-model、coflow-checker |
| C# 使用方式和版本 | runtimes/csharp/Coflow.Loader/、coflow-codegen-csharp、docs/integration/csharp-unity.md |
| 插件公开接口 | editors/cfd-editor/frontend/src/plugins/types.ts 和公开 wire 类型 |
| 打包安装 | src/skill_commands.rs |

新增能力时更新对应任务文件；涉及已有任务选择时同步更新入口路由。语言语义变化时检查两个技能中的相关概念和示例。网站章节重组不要求技能按相同结构改动。

## 示例和验证

最小项目包含完整 CFT、CFD 和配置，覆盖记录引用、singleton、默认值、Option、集合、维度与规则。结构变更示例新增无默认值的必填字段，候选类型与数据一起提供；候选放在项目输入目录外。

`tests/skills_examples.rs` 从实际 CLI 安装包提取技能到临时项目，再执行所附示例。验证默认值和引用、生成、批量范围保护、预览不落盘、冲突不发布、覆盖与回退、类型与数据联合更新和坏源码恢复。测试仅使用临时目录，不修改仓库示例或启动编辑器。

这些测试随 `cargo test --workspace` 执行；开发门禁仍为 AGENTS.md 规定的两条 Rust 命令。内容审查另外检查 frontmatter、本地链接、任务路由、与实现一致性及示例前提。

行为评测场景见 [技能任务评测](../../tests/skills/scenarios.md)。运行 Agent 评测时仅提供已安装技能、业务要求和隔离项目；不向执行者提供预期答案或实现路径。记录完成结果、额外澄清、错误命令及修复过程。CLI 示例通过与真实 Agent 任务成功分别报告。
