# 独立 CLI 进程性能基准

`cli-matrix.mjs` 使用 Node.js 内置库，在独立临时工程中调用明确指定的 CLI 可执行文件。每个样本启动新进程。输入包括结构化字段、数组、字典、内联对象、可选引用、字符串插值、语言和平台维度。

## 执行

```powershell
node scripts/performance/cli-matrix.mjs --list
node scripts/performance/cli-matrix.mjs --cli target/release/coflow.exe --case all --records 1000 --files 10 --samples 3 --warmups 1 --output results/cli-1000.json
node scripts/performance/cli-matrix.mjs --cli target/release/coflow.exe --case data_get,check_rules,set_field,batch_same_file,batch_cross_file --records 100000 --files 100 --batch 1000 --samples 3 --warmups 1 --output results/cli-100000.json
```

运行前构建 Release CLI，测量期间串行运行基准，避免编译、测试和其他基准争抢资源。

| 参数 | 含义 |
| --- | --- |
| `--case` | `all` 或逗号分隔的场景名称；`--list` 列出场景 |
| `--records` / `--files` | 记录与基础文件数量，记录数必须能被文件数整除 |
| `--fields` | 每条基础记录的字段数，至少 8，默认 20 |
| `--batch` | 批次目标数量，默认 1000；同文件批次受每文件记录数限制，实际数量记录于样本 `affected_records` |
| `--variants` | 已声明语言变体数量；大于零时另有一个平台变体 |
| `--coverage` | `sparse` 为每十条覆盖一条，`dense` 为全覆盖 |
| `--topology` | `sparse` 为二叉树引用，`high-fanin` 为全部引用首条，`none` 为无引用 |
| `--samples` / `--warmups` | 正式样本与预热数量，默认为 3 / 1 |
| `--timeout-ms` | 单个 CLI 调用的最长运行时间，默认 300000ms；超时样本计为失败，不作为已完成时延 |
| `--output` | 包含环境、工具/CLI/夹具 SHA256 与全部样本的 JSON 文件 |
| `--keep` | 保留生成的临时夹具；失败场景始终保留夹具 |
| `--prepare-only true` | 只创建并保留独立夹具，将目录、参数、字节数、首个数据文件路径及摘要写入输出；不启动 CLI |
| `--continue-on-error` | 一项失败后继续其他场景，最终进程仍以失败退出 |

## 操作覆盖

- 工程与 schema 检查，按 ID 读取、分页、筛选排序、搜索、反向引用和字段统计。
- 结构校验与业务规则校验。
- 单字段写入、带规则写入、同值写入、候选预览、同文件批次、跨文件批次、条件批次。
- 数组插入、移除与移动，字典设置与移除，记录创建、删除、重命名与迁移，移除显式字段。
- 维度读取、缺失覆盖查询、差异、多维组合查询、覆盖设置、新增变体及清除覆盖。
- 整文件写入、非法字段值拒绝、非法候选源拒绝、无效源修复及修订冲突拒绝。

每个场景开始前校验健康夹具。每次修改样本恢复原始输入，写入的变化值依次更新。计时内包含进程启动、业务执行、序列化、退出和标准输出收集；夹具生成、文件恢复、请求构造、JSON 解析及结果断言在计时外。

全部样本验证退出码、状态和变更结果；查询核对具体值、总数或排序等语义。每个修改样本在计时后通过新的 CLI 查询核对字段、集合、记录或维度结果，每个场景结束后另执行独立 `check --rules`。无变化、预览、非法候选和冲突场景还枚举完整实际输入集合，比较所有匹配源文件的路径与内容摘要，确认没有修改、删除或新增输入文件。

## 结果口径

`metrics[].samples_ms` 保留全部成功采样，`samples_detail` 保存参数、请求摘要、输出字节、退出码、状态、影响数量与诊断数量。P50 为算术中位数；少于 20 个样本时 P95 为 `null`。预热单独保留，不参与正式统计。

这是新进程、文件系统缓存已热的端到端测量。脚本没有清理操作系统文件缓存，也不把 Node 父进程的内存当作 CLI 内存；结果中的 `memory.collected` 明确为 `false`。实际进程峰值需由独立宿主采样工具测量。

独立 CLI 进程内存诊断可先复用相同夹具准备入口，再由宿主采样器直接启动 CLI：

```powershell
node scripts/performance/cli-matrix.mjs --prepare-only true --records 100000 --files 100 --fields 20 --output results/cli-memory-fixture.json
```

该入口输出 `root`、`config`、`source_bytes`、`fixture_sha256`、相对路径 `file_for_zero` 和绝对路径 `first_data_file`，不执行健康检查或任何 CLI 命令。调用方完成诊断后清理该临时目录。

宽度、覆盖率和引用拓扑改变夹具，结果必须同时记录参数与源字节数。同文件千项批次要求每文件至少 1000 条记录；不同文件数或不同语言变体数量的结果不能直接解释为同一操作的提速比例。
