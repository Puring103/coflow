# 设计缺陷与后续处理清单

本文档记录质量梳理过程中发现的设计级问题。这里的问题不一定需要在当前批次立即修复，但后续重构应持续消化。

## Rust LSP 与 VS Code 插件存在语义重复

当前 Rust LSP 已实现补全、hover、definition、document symbol、formatting 和 semantic tokens。VS Code 插件中仍保留较多本地解析、符号收集和语义 fallback 逻辑。

影响：

- CFT/CFD 语义规则变更后，Rust 和 JS 两套实现可能出现行为漂移。
- 插件维护成本高，测试需要覆盖重复逻辑。
- 用户遇到 LSP 启动失败时，插件 fallback 可能给出与真实语言服务不同的结果。

建议方向：

- Rust LSP 作为唯一语义事实来源。
- VS Code 插件只保留 LSP 进程管理、协议转换、错误展示和最小降级提示。
- 在移除 fallback 前，先补齐 LSP 行为测试，避免编辑器能力回退。

## 部分生产源码文件过大

当前重点文件包括：

- `crates/coflow-lsp/src/lib.rs`
- `editors/vscode-coflow/src/extension.js`
- `crates/coflow-codegen-csharp/src/lib.rs`
- `crates/coflow-data-model/src/compiler.rs`

影响：

- 单文件承载过多职责，代码审查和定位问题成本较高。
- 模块边界不够清楚，难以建立更细粒度测试。

建议方向：

- LSP 拆分为 protocol、diagnostics、completion、hover、definition、semantic tokens、formatting、URI/path helpers。
- VS Code 插件拆分为 session、providers、fallback 或 failure UI、path/config helpers。
- codegen 拆分为 preflight、schema-to-IR、render、per-format runtime glue。
- data model compiler 拆分为 record validation、index build、reference/path resolution、diagnostic mapping。

## 错误码覆盖标准尚未统一到所有模块

CFT 已有错误码覆盖测试，但“每个错误码都必须有负向触发测试和相邻合法输入不误报测试”的标准还没有统一覆盖到所有错误码体系。

影响：

- 新增错误码时可能只证明能触发，不能证明不会误报。
- 不同模块的错误码测试强度不一致。

建议方向：

- 为 CFT、cell value、CFD text、data model、checker、Excel loader、pipeline/artifact/codegen 逐步建立错误码覆盖清单。
- 对每个错误码记录：触发样例、相邻合法样例、期望位置和消息要点。

## 生成物和 lockfile 策略需要进一步明确

普通生成物已被忽略，但 `examples/humanpark/generated/csharp/coflow.enum.lock.json` 作为 `@keyAsEnum` 稳定值 lockfile 被跟踪。

影响：

- “generated 目录忽略但其中有跟踪文件”的策略容易造成维护混淆。
- 后续新增示例或变更生成路径时可能误删 lockfile。

建议方向：

- 在示例或项目文档中明确：普通生成文件不提交，`coflow.enum.lock.json` 可作为稳定枚举值输入提交。
- 长期可以考虑把 lockfile 输出位置从普通生成目录中分离出来。

