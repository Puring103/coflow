# Engine 与 Studio 目录边界

## 目录与依赖

仓库使用一个 Cargo workspace。根目录保留公共文档、网站、示例、发布工作流和共享配置。

| 目录 | 职责 |
| --- | --- |
| `engine/crates/coflow-language` | CFT/CFD 词法、语法、源码位置和结构限制 |
| `engine/crates/coflow-diagnostics` | 语言和执行共用的诊断分类 |
| `engine/crates/coflow-core` | 契约、声明编译、数据模型、内存加载、函数编译、VM、Host 与检查 |
| `engine/crates/coflow-ffi` | 原生 C ABI、句柄和回调适配 |
| `engine/runtimes/csharp` | C# 运行时封装及其测试、基准和集成样例 |
| `engine/tests/fixtures` | Engine 与 Studio 共用的语言一致性测试数据 |
| `studio/crates/coflow-project` | 项目配置、路径、文件发现、项目会话、检查编排、变更和发布 |
| `studio/crates/coflow-format` | 源码格式化 |
| `studio/crates/coflow-codegen*` | 代码生成契约与目标语言生成器 |
| `studio/crates/coflow-lsp` | 语言服务 |
| `studio/crates/coflow-staging` | 文件原子暂存 |
| `studio/cli` | `coflow` 命令行应用 |
| `studio/editors` | CFD 编辑器与 VS Code 扩展 |

Studio 依赖 Engine。Engine 的本地依赖全部位于 `engine/` 内；外部宿主通过内存输入和公开接口使用引擎。
依赖边界测试随 `cargo test --workspace` 执行，并检查可选依赖及平台依赖的本地路径。

## 源码与运行时

CFT/CFD 的语法、语义与编译属于 Engine。Studio 发现源文件、提供内容、管理修改及写回。
`coflow-core::loading` 提供严格解析和保留诊断的部分分析接口，项目层在其结果上补充文件来源、
行列位置和项目诊断。底层语法到数据的转换实现保持在 Engine 内部。

业务宿主加载契约字节并提交 CFD 文本。Engine 解析记录、补齐默认值、链接引用、编译函数并创建运行时。
CFD 解析与函数编译是运行时构建的一部分。CFT 声明编译通过 `cft-compiler` feature 选择启用。

## 独立构建与交付

在仓库根目录验证独立生产构建：

```powershell
pwsh engine/package-runtime.ps1 -Check
pwsh engine/package-runtime.ps1 -Check -CftCompiler
```

脚本把 Engine 的生产源码复制到临时 workspace，使用根 workspace 的 lint 和 release profile 设置，
以及同一份锁文件解析依赖。临时 workspace 不包含 Studio；生产 manifest 排除测试依赖与 benchmark target。
原生库通过明确的 `coflow-ffi` 包入口单独构建，构建缓存位于 `target/engine-runtime`。

完成当前版本对应的仓库发布门禁后，生成运行时交付包：

```powershell
pwsh engine/package-runtime.ps1
pwsh engine/package-runtime.ps1 -Target x86_64-pc-windows-msvc
```

`-CftCompiler` 生成额外启用 CFT 声明编译的版本。`-OutputDirectory` 指定交付目录，默认使用 `dist/engine`。
输出名称包含版本、目标平台及编译器变体标识，已有同名文件不会被覆盖。

运行时 ZIP 的内容使用白名单：`native/` 中的动态库、静态库及 C 头文件；`csharp/` 中的运行时源码、
Unity 包元数据、项目文件及使用说明；根目录许可证。开发工具、测试、基准、缓存和调试符号不进入交付包。
业务生成代码与项目契约由 Studio 的代码生成流程提供。

CLI 保持根目录 `cargo run -- ...` 的使用方式，本地安装入口为 `cargo install --path studio/cli --force`。
