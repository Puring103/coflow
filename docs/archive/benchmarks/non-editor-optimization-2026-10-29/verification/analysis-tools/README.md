# 最终证据复算工具

三个汇集与表格脚本使用 Node.js 内置模块分析已有归档。归档副本接受可选的 `archive-root` 参数；不传参数时，从仓库工作目录解析 `docs/archive/benchmarks/non-editor-optimization-2026-10-29`。从其他目录运行时传入归档的绝对路径即可。副本只调整入口路径解析，原分析逻辑和产物格式保持一致。

全部原始计划文件保留。总计划与补充计划可重复引用同一 ID 的完全相同规格，同一来源路径的原始文件只复制和计数一次；不同规格或不同来源的同名文件仍会拒绝。独立进程数量按唯一规格计数，重复引用不会增加样本或配对轮数。

| 文件 | 输入 | 产物 |
| --- | --- | --- |
| `consolidate-final.mjs` | `final-r4`、`managed-r6`、`cli-expanded-r4` 的计划及已退出原始证据 | 新建 `final-results`，逐字节复制并记录 `evidence-provenance.json`；输出目录须尚不存在。 |
| `audit-final-serial.mjs` | `final-results` 291 份、`stability-r4` 6 份、`cli-memory-r4` 24 份、`calibration-r5` 18 份、`calibration-r6` 14 份进程元数据 | `verification/final-serial-audit.json`；核对 353 个区间，采用 2 ms 舍入容差。 |
| `build-report-tables.mjs` | `final-results/{summary,comparison,target-status,evidence-provenance}.json` 及 `cli-memory-r4/cli-memory-report.json` | `results-tables.md`；检查完整计划、输入摘要、39 项 CLI 目标归属与 24 份独立内存元数据后生成表格。 |
| `sha256-manifest.json` | 归档分析脚本 | 文件字节数、SHA256，以及对应原件的路径与 SHA256。 |

在仓库根目录设置路径：

```powershell
$analysisArchive = 'docs/archive/benchmarks/non-editor-optimization-2026-10-29'
$analysisTools = Join-Path $analysisArchive 'verification/analysis-tools'
```

首次汇集完整原始证据时执行下列命令；已有 `final-results` 时直接使用该目录。

```powershell
node "$analysisTools/consolidate-final.mjs" "$analysisArchive"
```

基础统计与目标报告由仓库的既有分析器生成，执行顺序如下。这些命令仅读取已有测量并写出分析产物。

```powershell
node scripts/performance/summarize.mjs "$analysisArchive/final-results"
node scripts/performance/compare-optimization.mjs "$analysisArchive/final-results"
node scripts/performance/audit-optimization.mjs "$analysisArchive/final-results"
node scripts/performance/report-optimization-targets.mjs "$analysisArchive/final-results"
node scripts/performance/report-cli-memory.mjs "$analysisArchive/cli-memory-r4"
node "$analysisTools/audit-final-serial.mjs" "$analysisArchive"
node "$analysisTools/build-report-tables.mjs" "$analysisArchive"
```

独立 CLI 内存分析器还支持 `--baseline-sha256` 和 `--optimized-sha256`，可传入冻结 CLI 产物摘要绑定预期版本。目标状态与独立审计联合阅读；失败和截尾是完整的失败证据，缺少退出元数据表示证据尚未齐备。`stability-r4` 与校准证据仅参与串行区间审计，正文性能表使用正式三轮。

当前工作区及产物冻结校验从仓库根目录运行：

```powershell
node "$analysisArchive/verification/verify-final-freeze.mjs" "$analysisArchive"
```

该校验读取本机测量计划绑定的产物路径，检查 HEAD／分支、132 项源码与快照、功能收据与正式计划的产物对应，以及全部原始证据和来源映射。它要求这些本地产物仍存在且未变化；仅从归档复算统计不需要重新执行此项。

只检查归档脚本语法时：

```powershell
node --check "$analysisTools/consolidate-final.mjs"
node --check "$analysisTools/audit-final-serial.mjs"
node --check "$analysisTools/build-report-tables.mjs"
```

SHA256 清单仅绑定这些分析脚本，不替代生产源码冻结清单或测量载荷摘要。分析输出中的时间戳会随复算时间变化，原始测量、样本身份和阈值保持不变。

`verify-report-progress-events.mjs` 是另附的报告回归验证器，从仓库根目录运行，不接受参数。它只在 `target/performance-worklogs/analysis-regression/` 中创建证据副本，验证进度事件与真实指标的区分，以及真实重复、未知事件和缺失样本的拒绝；不运行被测程序。已完成的验收收据保存在 `verification/report-progress-event-regression.json`，其中 288 份原始输入及预算摘要保持一致。
