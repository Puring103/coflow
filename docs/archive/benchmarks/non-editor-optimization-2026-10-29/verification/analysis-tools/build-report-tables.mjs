// 只读取最终归档，手工执行后生成正文表；不启动基准、构建、测试或自动更新 README。
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

// 默认从仓库工作目录解析；显式参数可在任意目录指向完整归档。
assert.ok(process.argv.length <= 3, 'Usage: node build-report-tables.mjs [archive-root]');
const archive = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29');
const final = path.join(archive, 'final-results');
const inputFiles = {
  summary: path.join(final, 'summary.json'),
  comparison: path.join(final, 'comparison.json'),
  targets: path.join(final, 'target-status.json'),
  memory: path.join(archive, 'cli-memory-r4/cli-memory-report.json'),
  provenance: path.join(final, 'evidence-provenance.json'),
};
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const digest = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
for (const file of Object.values(inputFiles)) assert.ok(fs.existsSync(file), `Final evidence is not ready: ${file}`);
const summary = read(inputFiles.summary), comparison = read(inputFiles.comparison);
const report = read(inputFiles.targets), memory = read(inputFiles.memory);
const provenance = read(inputFiles.provenance);
for (const [directory, groups] of [['final-r4', ['rust', 'model', 'cli']], ['managed-r6', ['csharp', 'mono']], ['cli-expanded-r4', ['cli']]]) {
  assert.ok(provenance.sources?.some(source => source.directory === directory && groups.every(group => source.groups.includes(group))),
    `Final source provenance is incomplete: ${directory}`);
}
assert.ok(Array.isArray(summary.rows) && Array.isArray(summary.runs) && Array.isArray(summary.failures));
assert.ok(Array.isArray(comparison.comparisons) && Array.isArray(report.targets) && Array.isArray(report.process_peak_targets));

// 成功、失败和截尾都是已结束证据；缺少退出元数据表示尚未采样完成。
const plans = new Map();
const planFiles = fs.readdirSync(final).filter(name => /^plan-.*\.json$/u.test(name));
assert.ok(planFiles.length, 'Final plans are missing');
for (const file of planFiles) for (const spec of read(path.join(final, file)).plans) {
  // 总计划与补充计划中的相同引用只计为一个独立进程。
  if (plans.has(spec.id)) assert.deepEqual(plans.get(spec.id), spec, `Conflicting final specification: ${spec.id}`);
  else plans.set(spec.id, spec);
}
const runs = new Map();
for (const run of summary.runs) {
  const id = run.spec?.id ?? run.id;
  assert.ok(id && !runs.has(id), `Missing or duplicate process identity: ${id}`);
  assert.ok(Number.isInteger(run.exit_code) && typeof run.timed_out === 'boolean', `Process has not exited: ${id}`);
  runs.set(id, run);
}
assert.equal(runs.size, plans.size, 'Summary must contain every final planned process exactly once');
for (const [id, spec] of plans) assert.deepEqual(runs.get(id)?.spec, spec, `Missing or changed final process: ${id}`);
for (const key of ['summary', 'comparison']) {
  const captured = report.inputs?.filter(input => path.basename(input.path) === path.basename(inputFiles[key])) ?? [];
  assert.equal(captured.length, 1, `Target report is not bound to ${key}`);
  assert.equal(captured[0].sha256.toLowerCase(), digest(inputFiles[key]), `Target report is stale: ${key}`);
}
assert.equal(memory.expected_sampled_processes, 24);
assert.equal(memory.observed_process_metadata, 24, 'All 24 direct CLI processes must have exited before table generation');
assert.equal(memory.observations?.length, 24);
assert.ok(memory.observations.every(row => row.metadata_present), 'Direct CLI process metadata is incomplete');
assert.equal(memory.results?.length, 4);
assert.equal(memory.fixture_verifications?.length, 12);

const targetById = new Map(report.targets.map(target => [target.id, target]));
assert.equal(targetById.size, report.targets.length, 'Duplicate target identity');
const cliOperations = [
  'project_inspect', 'schema_inspect', 'data_get', 'data_list', 'filter_sort', 'data_search', 'data_refs',
  'check_structure', 'check_rules', 'stats_field', 'dimension_get', 'dimension_missing', 'dimension_diff', 'dimension_query',
  'set_field', 'set_field_rules', 'set_noop', 'set_preview', 'batch_same_file', 'batch_cross_file', 'set_where',
  'array_insert', 'array_remove', 'array_move', 'dict_set', 'dict_remove', 'create_record', 'delete_record',
  'rename_record', 'transfer_record', 'unset_field', 'dimension_set', 'dimension_add_variant', 'dimension_clear',
  'source_write', 'reject_invalid_value', 'reject_invalid_source', 'repair_invalid_source', 'revision_conflict',
];
const common = new Set(['data_get', 'check_rules', 'set_field', 'set_field_rules', 'set_noop', 'set_preview', 'batch_same_file', 'batch_cross_file']);
const cliTargets = cliOperations.map(metric => {
  const scenario = common.has(metric) ? 'cli-common-10k' : 'cli-expanded-10k';
  const found = report.targets.filter(target => target.scenario === scenario && target.metric === metric);
  assert.equal(found.length, 1, `Missing final CLI target mapping: ${metric}`);
  for (const arm of ['baseline', 'optimized']) for (const round of [1, 2, 3]) {
    assert.ok(plans.has(`${scenario}-r${round}-${arm}`), `Final CLI evidence is not complete: ${scenario}/${arm}/${round}`);
  }
  return found[0];
});
assert.equal(cliTargets.length, 39);

const finite = value => typeof value === 'number' && Number.isFinite(value);
const median = values => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const number = value => !finite(value) ? '—' : value === 0 ? '0' : Math.abs(value) < 0.01 ? value.toFixed(6) : value.toFixed(3);
const esc = value => String(value ?? '—').replaceAll('|', '\\|').replaceAll('\n', '<br>');
const status = value => ({ passed: '达标', not_met: '未达标', insufficient_evidence: '证据不足', not_applicable: '不适用' }[value] ?? value ?? '—');
const statuses = targets => Object.fromEntries(['passed', 'not_met', 'insufficient_evidence'].map(value => [value, targets.filter(target => target.status === value).length]));
const statusSentence = counts => `达标 ${counts.passed}，未达标 ${counts.not_met}，证据不足 ${counts.insufficient_evidence}`;
const link = target => `[${target.id}](final-results/target-status.json)`;
const lookup = id => {
  assert.ok(targetById.has(id), `Expected target is missing: ${id}`);
  return targetById.get(id);
};
const timeCriterion = target => target.criteria.find(item => item.key === 'median_ms' && item.kind === 'relative')
  ?? target.criteria.find(item => item.key === 'median_ms');
const successfulRun = source => {
  const id = path.basename(source).replace(/\.stdout\.ndjson$|\.results\.json$/u, '');
  const run = runs.get(id);
  return run?.exit_code === 0 && run?.timed_out === false;
};
const unitName = key => ({ median_ms: 'P50 ms', p95_ms: '观察 P95 ms', median_allocated_bytes: '分配 MB',
  median_allocated_current_thread_bytes: '当前线程分配 MB', median_retained_delta_bytes: 'GC 后保留增量 MB',
  sampled_process_peak_working_set_bytes: '进程峰值 MB' }[key] ?? key);
const scaleOf = key => key.includes('bytes') ? 1e6 : 1;
function pairedValues(criterion) {
  return (criterion?.pairs ?? []).filter(pair => finite(pair.baseline) && finite(pair.optimized));
}
function validEvidence(target, criterion, arm) {
  const excluded = new Set((criterion?.excluded_observations ?? []).map(row => row.source));
  return (target.evidence ?? []).filter(row => row.arm === arm && !row.problems?.length && !excluded.has(row.source) && successfulRun(row.source));
}
function valuesFor(target, criterion, arm) {
  if (!criterion) return [];
  const pairs = pairedValues(criterion);
  if (pairs.length) return pairs.map(pair => pair[arm]);
  // 基线超时但优化侧完成时，展示独立优化观测；这些值不生成相对提速。
  if (['median_ms', 'p95_ms'].includes(criterion.key)) return validEvidence(target, criterion, arm)
    .map(row => row[criterion.key]).filter(finite);
  return [];
}
function formatValues(values, scale = 1, fallback = null) {
  if (!values.length) return finite(fallback) ? `${number(fallback / scale)}（范围缺测）` : '—';
  return `${number(median(values) / scale)} [${number(Math.min(...values) / scale)}–${number(Math.max(...values) / scale)}]`;
}
function observation(target, criterion, arm) {
  return formatValues(valuesFor(target, criterion, arm), scaleOf(criterion?.key ?? ''), criterion?.[arm]);
}
function improvement(criterion) {
  const pairs = pairedValues(criterion);
  if (pairs.length < 3) return '—';
  const before = median(pairs.map(pair => pair.baseline)), after = median(pairs.map(pair => pair.optimized));
  return before > 0 ? `${number((1 - after / before) * 100)}%` : '—';
}
function criterionDescription(criterion) {
  const condition = criterion.kind === 'relative' ? `优化/基线 ≤${number(criterion.limit)}`
    : `≤${number(criterion.limit / scaleOf(criterion.key))} ${criterion.key.includes('bytes') ? 'MB' : criterion.unit}`;
  return `${unitName(criterion.key)} ${condition}${criterion.only_if_baseline_within != null ? `（基线 ≤${number(criterion.only_if_baseline_within)} ms 时）` : ''}`;
}
function acceptedValue(criterion) {
  if (!finite(criterion.observed)) return '—';
  return criterion.kind === 'relative' ? number(criterion.observed) : number(criterion.observed / scaleOf(criterion.key));
}
function stability(scenario, metric, key = 'median_ms') {
  const candidates = report.targets.filter(target => target.scenario === scenario && target.metric === metric);
  const match = candidates.flatMap(target => target.criteria.map(criterion => ({ target, criterion })))
    .find(({ criterion }) => criterion.kind === 'relative' && criterion.key === key && criterion.limit === 1.05);
  if (!match) return '无该项数值判定';
  return `${status(match.criterion.status)}（${acceptedValue(match.criterion)}；${match.criterion.paired_rounds} 对）`;
}
const lines = [];
const paragraph = text => lines.push(text, '');
const heading = text => lines.push(`## ${text}`, '');
function table(headers, rows) {
  lines.push(`| ${headers.map(esc).join(' | ')} |`, `| ${headers.map(() => '---').join(' | ')} |`);
  for (const row of rows) lines.push(`| ${row.map(esc).join(' | ')} |`);
  lines.push('');
}
function timingRow(target, label = target.id) {
  const criterion = timeCriterion(target);
  return [label, observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'), improvement(criterion),
    `${criterion?.paired_rounds ?? 0} 对 / ${criterion?.optimized_processes ?? 0} 优化进程`,
    target.criteria.map(criterionDescription).join('<br>'), `${status(target.status)} · ${link(target)}`];
}

paragraph('# 非编辑器性能最终结果表');
paragraph('本表使用最终冻结产物的正式证据：Rust／模型／CLI 为 R4，.NET／Unity Mono 为 R6。耗时单位为毫秒；`中位数 [最小–最大]` 的范围来自独立进程 P50，至少三对有效进程时采用同一组配对两侧统计。基线缺失时，优化侧可展示成功独立进程范围，相对收益保持空白。降低比例为 1 − 优化侧中位数 / 基线侧中位数；负值表示变慢。');
paragraph('状态直接引用 [目标验收](final-results/target-status.md)；5% 稳定性使用其独立条件。原始 [描述性比较](final-results/comparison.json) 中的部分失败运行不用于判定。超时、缺失和 Mono 不支持的分配计数均保留为空。MB 按十进制字节。查询批次、首次视图及零分配指标的单位在对应表中单列。');
table(['证据集合', '状态'], [
  ['数值目标', statusSentence(statuses(report.targets))],
  ['基准进程峰值', statusSentence(statuses(report.process_peak_targets))],
  ['独立 CLI 内存', statusSentence(statuses(memory.results))],
  ['最终计划与退出元数据', `${plans.size} 个计划，${runs.size} 份已退出进程元数据`],
]);
paragraph('以上是逐项数值证据状态。G1–G13 的算法工作量、阶段时延、功能回归和 IL2CPP 宿主状态另见主报告与 [方法审查](verification/final-report-method-review.md)。`stability-r4` 补测独立报告，未混入本表。');

heading('Rust Runtime 与模型');
const rustRows = [
  ['rust-node-budget', '单文件 2 万条节点预算拒绝'], ['rust-invalid-10k', '万条环形模板非法值'], ['rust-invalid-100k', '十万条环形模板非法值'],
  ['rust-load_checks', '十万条健康加载（规则开启）'], ['rust-load_no_checks', '十万条健康加载（无规则）'],
  ['rust-hot-scalar', '十万条热标量修改'], ['rust-grouped-1000', '十万条同文件 1,000 项修改'], ['rust-interleaved-1000', '十万条跨文件 1,000 项修改'],
  ['rust-sparse-dimension_existing_set', '万条十变体稀疏已有覆盖修改'], ['rust-sparse-dimension_existing_clear', '万条十变体稀疏已有覆盖清除'],
  ['rust-dense-dimension_existing_set', '万条十变体密集已有覆盖修改'], ['rust-dense-dimension_existing_clear', '万条十变体密集已有覆盖清除'],
  ['rust-projection', '万条稀疏一变体完整投影'], ['rust-projection-same', '万条稀疏一变体相同选择'],
  ...['insert_record', 'delete_record', 'rename_record', 'transfer_record'].flatMap(metric => [
    [`rust-${metric}`, `十万条含模板 ${metric}`], [`rust-literal-${metric}`, `十万条无模板 ${metric}`],
  ]),
];
table(['场景 / 操作', '基线 P50 [范围]', '优化 P50 [范围]', '降低比例', '有效配对 / 优化进程', '独立条件', '目标状态'], rustRows.map(([id, label]) => timingRow(lookup(id), label)));
const hot = lookup('rust-hot-scalar'), hotP95 = hot.criteria.find(criterion => criterion.key === 'p95_ms');
paragraph(`热标量观察 P95：基线 ${observation(hot, hotP95, 'baseline')} ms，优化 ${observation(hot, hotP95, 'optimized')} ms，${status(hotP95.status)}；每个贡献进程至少 20 个原始样本。P95 范围描述各进程观察 P95，不能作为稳定尾延迟保证。`);
table(['加载 / 恢复', '独立 5% 稳定性'], [
  ['十万条含规则加载', stability('rust-main-100k', 'load_checks')], ['十万条无规则加载', stability('rust-main-100k', 'load_no_checks')],
  ['万条错误后恢复', stability('rust-recovery-10k', 'error_recovery')],
]);

heading('CLI 基础 39 项操作');
const cliCovered = cliTargets.filter(target => (timeCriterion(target)?.paired_rounds ?? 0) >= 3).length;
const cliGroups = [
  ['原有核心操作', cliTargets.filter(target => target.scenario === 'cli-common-10k')],
  ['补充常见操作', cliTargets.filter(target => target.scenario === 'cli-expanded-10k')],
];
paragraph(`万条基础矩阵固定为 10 文件、20 字段、语言／平台各一变体，共登记 39 项操作；其中 ${cliCovered} 项具有三对有效耗时证据。common 场景承载 8 项，expanded 场景承载其余 31 项。计划、退出元数据和验收成功分别计数，完成补采不自动表示达标。千条基准批次为 100 项，万条批次和 set_where 预算绑定实际 1,000 项。`);
table(['场景', '操作数', '具备三对耗时', '数值目标状态'], cliGroups.map(([label, targets]) => [label, targets.length,
  targets.filter(target => (timeCriterion(target)?.paired_rounds ?? 0) >= 3).length, statusSentence(statuses(targets))]));
table(['万条操作', '基线 P50 [范围]', '优化 P50 [范围]', '降低比例', '有效配对 / 优化进程', '目标状态', '条件 5% 稳定性'], cliTargets.map(target => {
  const criterion = timeCriterion(target);
  return [target.metric, observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'), improvement(criterion),
    `${criterion?.paired_rounds ?? 0} / ${criterion?.optimized_processes ?? 0}`, `${status(target.status)} · ${link(target)}`, stability(target.scenario, target.metric)];
}));
paragraph('同值、预览和安全批次沿用专项绝对预算；一般操作的 5% 条件仅在基线已满足相应绝对预算时适用。“不适用”不是稳定性通过。各操作的预算、影响数量、排除原因和每轮值见链接中的目标记录。');

heading('.NET 与 Unity Mono 加载');
const managedFixtures = ['scalar20-1k', 'scalar20-100k', 'width8-10k', 'width100-10k', 'tables32-10k', 'complex-100k',
  'templates-10k', 'ring-10k', 'dim1-sparse-10k', 'dim10-sparse-10k', 'dim10-dense-10k', 'dim10-no-templates-10k'];
const managedRows = [];
for (const host of ['csharp', 'mono']) for (const fixture of managedFixtures) {
  const target = lookup(`stability-${host}-${fixture}:load_text`), criterion = timeCriterion(target);
  const dedicated = report.targets.filter(item => item.scenario === target.scenario && item.metric === 'load_text' && item.id !== target.id);
  managedRows.push([`${host === 'csharp' ? '.NET' : 'Mono'} / ${fixture}`, observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'),
    improvement(criterion), criterion.paired_rounds, `${status(criterion.status)}（${acceptedValue(criterion)}）`,
    dedicated.length ? dedicated.map(item => `${status(item.status)} · ${link(item)}`).join('<br>') : `稳定性 · ${link(target)}`]);
}
table(['宿主 / 夹具', '基线 P50 [范围]', '优化 P50 [范围]', '降低比例', '有效配对', '独立 5% 稳定性', '专项目标'], managedRows);
paragraph('标量十万条、宽表、复杂数据、维度加载的专项目标与 5% 回归条件分别判定。Mono 的十万条标量／复杂加载同时要求绝对预算和相对本宿主基线至少 40% 改善。');

heading('托管分配、保留内存与维度');
const managedMemoryRows = [];
for (const id of ['dotnet-scalar100k', 'dotnet-width100', 'dotnet-complex100k', 'dotnet-first-view']) {
  const target = lookup(id);
  for (const criterion of target.criteria.filter(item => item.key.includes('bytes'))) managedMemoryRows.push([
    id, unitName(criterion.key), observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'),
    criterionDescription(criterion), `${status(criterion.status)}；${criterion.paired_rounds} 对`, link(target),
  ]);
}
for (const host of ['csharp', 'mono']) for (const fixture of ['dim1-sparse-10k', 'dim10-sparse-10k', 'dim10-no-templates-10k']) {
  const target = lookup(`${host}-${fixture}`);
  for (const criterion of target.criteria.filter(item => item.key.includes('bytes'))) managedMemoryRows.push([
    `${host === 'csharp' ? '.NET' : 'Mono'} / ${fixture}`, unitName(criterion.key), observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'),
    criterionDescription(criterion), `${status(criterion.status)}；${criterion.paired_rounds} 对`, link(target),
  ]);
}
table(['场景', '指标', '基线 MB [范围]', '优化 MB [范围]', '条件', '判定 / 配对', '证据'], managedMemoryRows);
const firstView = lookup('dotnet-first-view');
table(['首次视图', '基线 ms [范围]', '优化 ms [范围]', '降低比例', '有效配对 / 优化进程', '独立条件', '目标状态'], [timingRow(firstView, '.NET 万条首次全表视图')]);
paragraph('分配表示被测操作的托管分配量；保留增量是 GC 后观察值。Mono 空分配计数不参与数值比较，也不等于零分配。维度加载相对收益和全表首次视图分配收益均使用本宿主、同夹具的有效配对。');

heading('托管查询与缓存维度');
const queryRows = [];
for (const host of ['csharp', 'mono']) for (const fixture of ['scalar20-100k', 'width100-10k', 'tables32-10k', 'ring-10k']) {
  for (const metric of ['table_lookup_batch', 'direct_property_batch', 'table_enumerate']) {
    const target = lookup(`stability-${host}-${fixture}:${metric}`), criterion = timeCriterion(target);
    const counts = [...new Set(target.evidence.map(row => row.operations_per_sample))];
    queryRows.push([`${host === 'csharp' ? '.NET' : 'Mono'} / ${fixture}`, metric, counts.join('/'),
      observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'), criterion.paired_rounds,
      `${status(criterion.status)}（${acceptedValue(criterion)}） · ${link(target)}`]);
  }
}
table(['宿主 / 夹具', '查询', '每样本操作数', '基线批次 ms [范围]', '优化批次 ms [范围]', '有效配对', '5% 稳定性'], queryRows);
const cacheRows = [];
for (const host of ['csharp', 'mono']) for (const metric of ['dimension_cached_for_batch', 'dimension_variants_batch', 'dimension_has_variant_batch']) {
  const target = lookup(`${host}-${metric}`), allocation = target.criteria.find(criterion => criterion.key.includes('bytes'));
  const timing = timeCriterion(target);
  cacheRows.push([host === 'csharp' ? '.NET' : 'Mono', metric, 10000,
    finite(allocation?.optimized) ? `${number(allocation.optimized)} B/批次` : '—（计数不可用）',
    allocation?.optimized_processes ?? 0, status(allocation?.status), timing ? `${observation(target, timing, 'optimized')} ms；${status(timing.status)}` : '无独立时延预算',
    `${status(target.status)} · ${link(target)}`]);
}
table(['宿主', '缓存操作', '每样本调用数', '优化分配最大值', '有效分配进程', '零分配判定', '时延预算', '目标状态'], cacheRows);
paragraph('缓存零分配条件使用所有贡献进程全部原始样本的最大值，每个样本为一万次调用；该条件比仅观察中位数为零更严格。查询计时仍按完整批次展示。');

heading('独立 CLI 进程内存');
paragraph(`直接采样 ${memory.observed_process_metadata}/24 个命令进程，其中有效 ${memory.valid_sampled_processes}；夹具复核通过 ${memory.passed_fixture_verifications}/12。每项采用 AB/BA/AB 三对进程；基线与优化范围仅来自有效配对，排除样本保留在 [独立内存报告](cli-memory-r4/cli-memory-report.md)。`);
table(['命令 / 规模', '基线峰值 MB [范围]', '优化峰值 MB [范围]', '峰值比', '有效配对', '绝对预算 MB', '绝对 / 5% / 总状态'], memory.results.map(result => {
  const accepted = result.pairs.filter(pair => pair.valid);
  return [`${result.operation} / ${result.records}`, formatValues(accepted.map(pair => pair.baseline_bytes), 1e6),
    formatValues(accepted.map(pair => pair.optimized_bytes), 1e6), number(result.paired_peak_ratio), result.paired_rounds,
    number(result.criteria.find(criterion => criterion.kind === 'absolute')?.limit / 1e6),
    `${result.criteria.map(criterion => status(criterion.status)).join(' / ')} / ${status(result.status)}`];
}));
paragraph('该峰值属于单个 coflow.exe 命令，包含命令内加载、修改及输出；外部 Node／PowerShell、夹具生成及复核不计入。OS 累计工作集峰值约每 100 ms 观察，最后区间可能缺测；此表不推导分配量、常驻堆或时延分布。');

heading('未达标与证据不足');
function issueReason(target, criterion) {
  if (criterion.status === 'not_met') return '有效证据超过该条件阈值';
  const reasons = [];
  if (target.scenario.startsWith('il2cpp-')) reasons.push('缺少真实 IL2CPP Player 证据');
  if (criterion.optimized_processes < 3) reasons.push(`有效优化进程 ${criterion.optimized_processes}/3`);
  if (criterion.kind === 'relative' && criterion.paired_rounds < 3) reasons.push(`有效配对 ${criterion.paired_rounds}/3`);
  if (criterion.key.includes('bytes') && !finite(criterion.optimized)) reasons.push('分配／内存计数缺测或不支持');
  if (criterion.failed_processes?.length) reasons.push(`失败／截尾进程 ${criterion.failed_processes.length}`);
  if (criterion.rejected_sources?.length) reasons.push(`排除原始来源 ${criterion.rejected_sources.length}`);
  return reasons.join('；') || '有效证据不足，详细排除原因见目标记录';
}
for (const currentStatus of ['not_met', 'insufficient_evidence']) {
  paragraph(`**${status(currentStatus)}的数值条件**`);
  const rows = report.targets.flatMap(target => target.criteria.filter(criterion => criterion.status === currentStatus).map(criterion => [
    link(target), criterionDescription(criterion), acceptedValue(criterion), `${criterion.paired_rounds} 对 / ${criterion.optimized_processes} 优化进程`, issueReason(target, criterion),
  ]));
  if (rows.length) table(['目标', '条件', '验收观测值', '有效证据', '原因'], rows);
  else paragraph('无。');
}
paragraph('**未通过的进程峰值条件**');
const peakIssues = report.process_peak_targets.filter(target => target.status !== 'passed');
if (peakIssues.length) table(['场景', '基线峰值 MB [范围]', '优化峰值 MB [范围]', '峰值比', '有效配对', '状态 / 原因'], peakIssues.map(target => {
  const criterion = target.criteria[0];
  return [target.scenario, observation(target, criterion, 'baseline'), observation(target, criterion, 'optimized'), acceptedValue(criterion), criterion.paired_rounds,
    `${status(criterion.status)}；${issueReason(target, criterion)}`];
}));
else paragraph('所有已登记基准进程峰值目标均达标；其适用范围仍为对应基准进程。');
const endedWithFailure = [...runs.values()].filter(run => run.timed_out || run.exit_code !== 0);
paragraph(`最终证据保留 ${endedWithFailure.length} 个失败或截尾进程，CLI 操作失败记录 ${summary.failures.length} 条。进程失败与操作失败分别计数；时限覆盖整个进程，未折算成操作毫秒或收益倍率。`);
if (endedWithFailure.length) table(['进程', '退出状态', '时限口径'], endedWithFailure.map(run => [run.spec.id,
  run.timed_out ? '超时截尾' : `失败，退出码 ${run.exit_code}`, run.timed_out ? `整进程上限 ${run.timeout_seconds} 秒` : '—']));
paragraph('相对收益、绝对预算、功能能力及宿主完整性分别验收。剩余优化与补证据计划参见主报告；本表中的数量不表示所有算法目标已经实现。');

heading('输入与复核');
table(['输入', 'SHA256'], Object.entries(inputFiles).map(([key, file]) => [`[${key}](${path.relative(archive, file).replaceAll('\\', '/')})`, digest(file)]));
paragraph('本表只展示目标报告已给出的条件状态；目标报告与独立证据审计需联合阅读。三轮原值、夹具、影响数量、排除原因、原始输出及来源摘要由最终证据目录保留。');
const output = path.join(archive, 'results-tables.md');
fs.writeFileSync(output, `${lines.join('\n')}\n`);
console.log(JSON.stringify({ output, planned_processes: plans.size, cli_operations: cliTargets.length, cli_with_three_pairs: cliCovered,
  targets: statuses(report.targets), process_peaks: statuses(report.process_peak_targets), cli_memory: statuses(memory.results) }));
