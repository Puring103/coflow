# Coflow 设计与工程文档

本目录按职责维护当前实现的设计契约。使用教程与公开参考位于 [网站文档](../website/docs/docs/index.md)，开发与发布门禁以 [AGENTS.md](../AGENTS.md) 为准。

## 当前语言

- [语言规格总览](language-design/spec/01-总览.md)：CFT 声明、CFD 数据及共同语义，按主题导航。
- [语言实现边界](language-design/01-language-design.zh-CN.md)：语言、数据模型、检查器与宿主之间的职责。
- [语言核心实现契约](language-design/05-language-evolution.zh-CN.md)：默认值、身份、模板、规则任务、诊断与宿主协作。
- [源码格式化](language-design/04-source-formatting.zh-CN.md)：无损词法流、规范输出与宿主接入。

## 架构与运行时

- [系统架构](architecture/overview.md)：模块边界、项目流水线与错误处理。
- [项目代次与领域术语](architecture/project-generations.md)：项目快照、编辑器代次、变更计划与产物发布。
- [CLI 协议与执行设计](runtime-design/cli.md)：查询、结构化值、变更集、校验、事务与输出协议。
- [路径与产物发布](runtime-design/paths-and-publication.md)：路径身份、Git 比较与生成文本等价。
- [C# 生成与加载](language-design/02-api-runtime-design.zh-CN.md)：业务类型、描述符、引用、维度与模板加载。
- [C# 与 Unity 接入](integration/csharp-unity.md)：应用引用、加载和读取示例。

## 编辑器

- [视图与工作区](editor-design/views.md)：文件视图、维度导航、选择与恢复。
- [维度编辑](editor-design/dimensions.md)：覆盖来源、文件组织、写入与会话更新。
- [值创建](editor-design/value-creation.md)：默认值、初值、必填项及内联对象创建。
- [容错数据加载](editor-design/tolerant-loading.md)：可编辑错误、记录保留与严格消费者。
- [性能约束](editor-design/performance.md)：选区、索引、后端列统计、挂载与查询缓存。
- [记录展示](editor-design/record-display.md)：缩略名和记录标识。
- [Git Diff](editor-design/git-diff.md)：工作区与历史数据比较。
- [插件接口](editor-design/plugins.md)：注册、事件、数据访问和展示出口。
- [视觉规范](../assets/visual-style.md)：品牌、界面与图标的共同设计。

## 工程维护

- [质量要求](development/quality.md)：代码、诊断、测试、文档及检查职责。
- [引擎核心性能优化方案](development/engine-performance-plan.zh-CN.md)：十万条记录与高频编辑的优化目标、已实施结构、增量安全边界和验收方式。
- [发布流程](releasing.md)：版本、安装器、发布资产与更新签名。
- [macOS 签名](releasing-macos-signing.md)：证书、公证和发布环境配置。
- [VS Code 扩展](../editors/vscode-coflow/README.md)：语言服务接入和扩展开发。

## 历史资料

[归档索引](archive/README.md) 保存旧实施计划、界面草图和历史性能测量，供追溯原始资料。
