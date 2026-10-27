# 项目审查修复验收

日期：2026-10-28。基于0.14.0、`f1feade1f8c867a337a02be55c107c6f7c787109`工作区。本轮交付代码与文档修复、独立Skill评测及性能复测，纳入本地提交；不包含推送、安装CLI、打包或发布，未启动或停止桌面编辑器。

## 修复对应关系

| 编号 | 完成行为 | 主要回归证据 |
| --- | --- | --- |
| R1 | 加载器与输入快照共用发现规则；候选保留实际链接入口及实体映射 | `.cfd`链接到`.txt`，同时存在`.txt.cfd`；内容改变revision、旧修订冲突、重复ID拒绝、别名读写不破坏链接 |
| R2 | 文件锁覆盖完整设置读改写；前端串行保存并按目标合并 | 8个独立会话并发、4个独立进程屏障同步，成功更新全部保留 |
| R3 | watcher发送已提交bootstrap，前端直接接收代次 | core事件回归只推进一次；现有代次、LSP草稿与前端测试 |
| R4 | 全文搜索递归Option复杂值 | 嵌套Option集合命中叶路径、None不误命中 |
| R5 | 先分页再物化；请求级源码LineIndex与文件身份缓存 | 现有分页、游标、来源、Unicode位置回归随工作区测试通过；不宣称全局筛选为常数复杂度 |
| R6 | 预览、检查与发布共用生成文本等价规则 | 真实CLI CRLF/末尾换行检查、连续生成字节与mtime不变 |
| R7 | 按C#字符规则拒绝不可表示名称，保留失败发布约束 | ℘、补充平面名称拒绝，中文及关键词正常；实际Roslyn编译验证 |
| R8 | C#共享字符串出口转义U+2028/U+2029 | 生成、真实C#编译和加载逐值往返，含NEL |
| R9 | Loader使用由Rust生成的完整XID范围 | 每个Unicode标量逐项核对；共享语料℘、𐐀key、a·b在两端通过 |
| R10 | CLI统一执行span与终结事件，覆盖参数、LSP、普通响应和输出失败 | 实际子进程日志关联CLI-REQUEST与exit2；不记录原始参数，JSON不受污染 |
| R11 | 中英文安装资产选择与编辑器/VS Code上手 | README、指南、导航与网站构建 |
| R12 | 公开同版本Loader源码获取及项目引用步骤 | 独立Skill任务中仅CLI/.NET应用接入，成功及失败分支实际运行 |
| R13 | 英文补齐日志、反馈、可选详情及VS Code输出入口 | 中英文文档核对与网站构建 |
| R14 | reload与watcher保留结构诊断和publication信息 | 错误CFT刷新事件保留原诊断，前端统一错误记录 |
| R15 | 插件原始Error贯通，内存保留stack，跨层去重 | 插件异常隔离、原始Error断言、incident详情与去重、反馈导出浏览器回归 |

Rust通用门禁为 `cargo check --workspace`、`cargo test --workspace`，最终工作区均通过。本轮专项验证包含TS bindings生成、381项Vitest、前端生产构建、55项Loader测试、netstandard2.1 smoke、C#生成集成应用及网站构建。浏览器两轮串行均为91/92：首轮反馈测试遇到Vite依赖预构建热重载，次轮为外部数组删除后编辑草稿用例超时；两项分别独立重跑通过，数组用例连续两次独立重跑通过。浏览器整套仍存在时序不稳定，未报告为全绿。通用门禁不增加前端、C#或格式化要求。

## 网站主要特性覆盖

| 特性 | 公开入口 |
| --- | --- |
| 安装、完整示例、AI/CLI | `guide/install`、`reference/08-cli` |
| 编辑器多视图、撤销、Git比较、VS Code | `guide/editors` |
| 类型、继承、默认值、引用与规则 | `reference/03-language/01-cft` |
| 数据、集合、Option与模板 | `reference/03-language/02-cfd` |
| 维度与本地化 | `reference/10-localization` |
| C#类型生成与应用加载 | `reference/07-codegen/01-csharp` |
| 插件 | `reference/13-plugins` |
| 错误定位、日志与反馈 | `reference/09-diagnostics/01-diagnostics`及`02-codes` |

以上路径均位于`website/docs/docs/`，英文具有对应路径；公开页面只描述用户操作与行为。

## 独立评测与性能边界

八类任务的最终项目和原始命令见[Skill评测](../archive/skill-evaluation-2026-10-28/README.md)。插件为无头验证，真实编辑器加载、派发与视觉仍需专用宿主验收。未用CLI示例测试冒充Agent行为评测。

性能结果见[复测报告](../archive/benchmarks/review-performance-2026-10-28.md)：图首屏与稳态帧间隔未达目标；引擎热编辑P50未达，P95达到。历史相对收益与峰值内存不能由本次数据确认。图性能专项优化另行实施，既有目标与未达状态保持明确。
