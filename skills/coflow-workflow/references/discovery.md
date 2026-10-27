# 定位项目与发现能力

适用于首次进入项目、类型未知、命令不存在或 CLI 不可用。

## 确认工具与项目

已有可用 CLI 时直接使用。遇到命令缺失先查看 `coflow --version` 和 `coflow describe --json`。CLI 不在 PATH 时先检查用户指定的安装位置；需要安装时可从 [Coflow Releases](https://github.com/Puring103/coflow/releases) 获取对应平台发布物，或在已有 Rust 环境中运行：

```sh
cargo install --git https://github.com/Puring103/coflow.git coflow
```

安装、更新和平台选择沿用用户任务范围；发现能力差异时说明所需能力与当前版本。Windows 更新通过安装器。

项目命令显式使用 `--project <目录或 coflow.yaml>`。省略时从当前目录向父目录寻找配置，容易误选父项目。没有项目时 `coflow init <目录>` 创建配置和目录；新类型设计需要配套 CFT 与 CFD，init 本身不提供业务记录。

## 从未知类型开始

```sh
coflow project inspect --project <项目> --json
coflow project files --project <项目> --json
coflow schema symbols --project <项目> --kind type --json
coflow context --project <项目> --type Item --examples 3 --json
```

先确定 Item 确实存在，再运行最后一条。`project inspect` 检查配置，`project files` 列出实际输入；同一个项目中的 CFT 共享声明，无需 import。保存读取响应顶层 revision，供后续写入。

已知文件而未知内容时使用 `source read --file <项目相对路径>`；需要片段时 `--start-line`、`--end-line` 从零开始，含起点、不含终点。

## 发现命令和请求

```sh
coflow describe --json
coflow describe data.set --json
coflow describe apply --schema input --json
```

describe 参数使用 `data.set` 这样的点分命令名。先看简明参数和示例，只有构造复杂请求时读取 input Schema，解析复杂响应时读取 output Schema；定义在 `result.definition`。

完成发现后应能指出：项目根、相关类型、实际输入文件和下一步命令。配置或 Schema 损坏时，保留诊断并转向[校验与恢复](validation-and-recovery.md)，无需循环重试依赖完整模型的查询。
