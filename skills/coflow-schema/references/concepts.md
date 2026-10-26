# 建模所需的项目与 CLI 概念

coflow.yaml 选择 CFT、CFD、维度目录和生成目标。CFT 定义类型与只读业务规则，CFD 提供记录和覆盖。应用使用生成类型及 Loader 加载 CFD；业务 check 在 CLI 或编辑器中执行。

| 建模对象 | 身份与用法 |
| --- | --- |
| table | 多条独立记录，可通过 `&Table` 引用 |
| singleton | 恰好一条同名记录，用作全局设置 |
| data | 内联拥有值，通过所属字段访问 |
| enum | 固定分类；标志集合使用 @flag |

同一继承树中记录 ID 唯一；table/singleton 的 id 为固有身份，data 可有普通 id 字段。字段是否可空与是否可省略独立，详见[默认值与 Option](defaults-and-options.md)。

## 进入已有项目

```sh
coflow project inspect --project <项目> --json
coflow project files --project <项目> --json
coflow schema symbols --project <项目> --kind type --json
coflow context --project <项目> --type Item --examples 3 --json
coflow schema refs --project <项目> --symbol Item --json
```

context 需要类型存在；新类型先读取相关文件。按需用 schema inspect、rule list/inspect、source read 补足声明与规则。context 的样例不代表全部受影响记录；修改前用 data list 翻页核对，派生记录需要 include-derived。

数据查询的 data_scope.complete 表示加载完整性，列表 truncated 表示输出截断。数据不完整时不能据现有样例完成全范围迁移判断。保存响应顶层 revision，写入携带 if-revision。

## 工具与协议

```sh
coflow --version
coflow describe --json
coflow describe apply --schema input --json
```

describe 使用点分命令名，如 schema.inspect；完整参数和 input/output Schema 来自安装版本，定义在 result.definition。CLI 不可用时确认 PATH 与安装位置；需要安装可从 [Releases](https://github.com/Puring103/coflow/releases) 获取发布物，或使用 `cargo install --git https://github.com/Puring103/coflow.git coflow`。按用户任务范围处理安装和更新。

项目命令带 `--project <目录或 coflow.yaml>`；省略时向父目录搜索。配置和源码目标路径以项目根为基准，input 文件以调用目录为基准；变更集 content_file 以请求 JSON 所在目录为基准。

## 成功与失败

使用 --json 解析响应，result 中提供实际命令结果。退出码 0 为成功，1 为校验失败或结果不完整，2 为请求错误，3 为前置条件冲突，4 为 I/O 或执行失败。check 的 complete 与是否通过分别判断。

默认最多 50 条诊断，diagnostics_summary 报告截断；details 展开，显式 limit 仍有效。code、help、record、field_path 和 location 用于修复，`coflow explain <code> --json` 查原因。源码行列从零开始，列为 UTF-16 偏移。

请求错误可能含 request_path、operation_index、target。冲突后重读数据与范围；发布失败核对 written_files、restored_files、restoration_failed。完整的候选工作流见[结构变更](schema-migration.md)。
