// 目标验收只消费已有证据；失败、截尾、缺失和不支持的分配计数均不补成零。
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const directory = path.resolve(argv[0] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29/final-r3');
if (argv.length > 1 && (argv[1] !== '--output' || !argv[2] || argv.length !== 3)) throw new Error('Usage: node report-optimization-targets.mjs <results-directory> [--output <report-directory>]');
const output = path.resolve(argv[2] ?? directory);
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const summaryPath = path.join(directory, 'summary.json');
const comparisonPath = path.join(directory, 'comparison.json');
const summary = read(summaryPath), comparison = read(comparisonPath);
const budgetPath = path.join(repo, 'docs/archive/benchmarks/non-editor-2026-10-28/README.md');
const budgetLines = fs.readFileSync(budgetPath, 'utf8').split(/\r?\n/u);
const finite = value => typeof value === 'number' && Number.isFinite(value);
const median = values => {
  if (!values.length) return null;
  const xs = [...values].sort((a, b) => a - b), middle = Math.floor(xs.length / 2);
  return xs.length % 2 ? xs[middle] : (xs[middle - 1] + xs[middle]) / 2;
};
const equal = (a, b) => a === b || (finite(a) && finite(b) && Math.abs(a - b) < 1e-8 * Math.max(1, Math.abs(a), Math.abs(b)));
const source = phrase => {
  const index = budgetLines.findIndex(line => line.includes(phrase));
  if (index < 0) throw new Error(`Budget source missing: ${phrase}`);
  return { path: path.relative(repo, budgetPath).replaceAll('\\', '/'), line: index + 1, text: budgetLines[index] };
};
const sources = {
  invalid: source('| P0：环形模板非法值'), budget: source('| P0：单文件 2 万条预算拒绝'),
  load: source('| P1：Runtime 十万条健康加载'), cli: source('| P1：CLI 万条同值/预览/单字段'),
  hot: source('| P1：Runtime 十万条热标量'), grouped: source('| P1：同文件 1,000 项 Runtime 修改'),
  scalar: source('| P1：.NET 十万条 20 标量字段'), width: source('| P1：.NET 万条 100 字段'),
  cache: source('| P1：缓存维度查询'), interleaved: source('| P2：Runtime 跨百文件千项修改'),
  members: source('| P2：Runtime 十万条成员变化'), dimension: source('| P2：Runtime 已有维度文件修改/清除'),
  projection: source('| P2：Rust 完整维度投影'), cliBatch: source('| P2：CLI 安全独立批次'),
  complex: source('| P2：.NET 十万条复杂数据'), dimensionLoad: source('| P2：稀疏维度加载'),
  first: source('| P2：全表首次视图'), mono: source('| P3：Unity Mono / IL2CPP'),
  stability: source('已达到绝对预算的操作保持本轮 P50 与峰值稳定回归不超过 5%'),
  loadStability: source('| G5：加载/恢复与峰值回归 ≤5%'),
  hotRelative: source('| G2：热修改 50/100 ms，且改善至少 25%'),
};

const runs = new Map();
for (const run of summary.runs ?? []) {
  const id = run.spec?.id ?? run.id;
  if (!id) continue;
  if (!runs.has(id)) runs.set(id, []);
  runs.get(id).push(run);
}
const rawCache = new Map();
function rawEvidence(relative) {
  if (rawCache.has(relative)) return rawCache.get(relative);
  let result;
  try {
    const content = fs.readFileSync(path.resolve(directory, relative), 'utf8').replace(/^\uFEFF/u, '');
    let entries;
    try { entries = [JSON.parse(content)]; }
    catch { entries = content.split(/\r?\n/u).filter(line => line.trim().startsWith('{')).map(line => JSON.parse(line)); }
    // 进度事件携带 metric 名称，但不是聚合结果；仅跳过明确的开始/完成事件。
    // 其他指标继续完整校验，真实聚合重复或缺少样本仍必须拒绝。
    const metrics = entries.flatMap(entry => {
      if (entry.event === 'sample_started' || entry.event === 'sample_completed') return [];
      return entry.metrics ?? (entry.metric ? [entry] : []);
    });
    const metadata = entries.find(entry => entry.kind === 'metadata') ?? entries.find(entry => entry.config) ?? {};
    result = { entries, metrics, config: metadata.config ?? metadata, complete: entries.some(entry => entry.kind === 'complete' && entry.success === true) };
  } catch (error) { result = { error: String(error) }; }
  rawCache.set(relative, result);
  return result;
}
function fixtureSignature(config) {
  const allowed = new Set(['case', 'records', 'files', 'fields', 'variants', 'density', 'coverage', 'topology', 'templates', 'batch', 'collectionlength', 'collection_length', 'dimension_stride', 'querybatch', 'query_batch']);
  return JSON.stringify(Object.fromEntries(Object.entries(config).map(([key, value]) => [key.toLowerCase(), value]).filter(([key]) => allowed.has(key)).sort(([a], [b]) => a.localeCompare(b))));
}
function normalizedConfig(config) { return Object.fromEntries(Object.entries(config).map(([key, value]) => [key.toLowerCase(), value])); }

// 同轮重复文件不能算成独立进程；原始样本、退出状态及托管完成标记一起验证。
const observations = [];
for (const row of summary.rows ?? []) {
  const match = /^(.*)-r(\d+)-(baseline|optimized)$/u.exec(path.basename(row.run));
  if (!match) continue;
  const [, scenario, roundText, arm] = match, round = Number(roundText);
  const candidates = runs.get(path.basename(row.run)) ?? [];
  const run = candidates.length === 1 ? candidates[0] : null;
  const problems = [];
  if (!run) problems.push(candidates.length ? 'duplicate_process_metadata' : 'missing_process_metadata');
  else {
    if (run.exit_code !== 0 || run.timed_out !== false) problems.push('failed_or_censored_process');
    if (run.spec?.scenario !== scenario || run.spec?.arm !== arm || run.spec?.round !== round) problems.push('process_identity_mismatch');
  }
  const raw = rawEvidence(row.source);
  const metrics = raw.metrics?.filter(metric => metric.metric === row.metric && (metric.records == null || metric.records === row.records) && (metric.fields == null || metric.fields === row.fields)) ?? [];
  const metric = metrics.length === 1 ? metrics[0] : null;
  if (raw.error) problems.push('unreadable_raw_evidence');
  if (!metric) problems.push(metrics.length ? 'duplicate_raw_metric' : 'missing_raw_metric');
  else {
    if (metric.correctness != null && metric.correctness !== 'passed') problems.push('correctness_not_passed');
    if (!Array.isArray(metric.samples_ms) || !metric.samples_ms.length || !metric.samples_ms.every(value => finite(value) && value >= 0)) problems.push('invalid_raw_samples');
    else {
      if (metric.samples_ms.length !== row.samples || !equal(median(metric.samples_ms), row.median_ms)) problems.push('summary_differs_from_raw');
      const sorted = [...metric.samples_ms].sort((a, b) => a - b);
      if (!equal(row.p95_ms, sorted.length >= 20 ? sorted[Math.ceil(sorted.length * .95) - 1] : null)) problems.push('summary_p95_differs_from_raw');
    }
    if ((metric.operations_per_sample ?? (/^model_clone_10000_f/u.test(metric.metric) ? 10000 : 1)) !== row.operations_per_sample) problems.push('operation_count_mismatch');
  }
  if (['csharp', 'mono'].includes(run?.spec?.group) && !raw.complete) problems.push('managed_completion_missing');
  if ((summary.failures ?? []).some(failure => failure.source === row.source && failure.metric === row.metric)) problems.push('reported_operation_failure');
  const affected = metric?.samples_detail?.map(sample => sample.affected_records);
  observations.push({ ...row, scenario, round, arm, affected_records: affected?.length && affected.every(value => finite(value) && value === affected[0]) ? affected[0] : null, run_metadata: run, raw_metric: metric, fixture_config: normalizedConfig(raw.config ?? {}), fixture_signature: fixtureSignature(raw.config ?? {}), problems });
}
for (const observation of observations) {
  if (observations.filter(other => other.scenario === observation.scenario && other.metric === observation.metric && other.records === observation.records && other.fields === observation.fields && other.operations_per_sample === observation.operations_per_sample && other.round === observation.round && other.arm === observation.arm).length > 1) observation.problems.push('duplicate_round_metric');
}

const targets = [];
const absolute = (key, limit, unit = 'ms', extra = {}) => ({ kind: 'absolute', key, operator: '<=', limit, unit, ...extra });
const relative = (key, improvement, extra = {}) => ({ kind: 'relative', key, operator: '<=', limit: 1 - improvement / 100, unit: 'optimized/baseline', minimum_improvement_percent: improvement, ...extra });
const stable = (key = 'median_ms') => relative(key, -5);
function add(id, scenario, metric, reference, criteria, expected = {}, note = null) {
  const fixtureExpected = {};
  if (scenario.startsWith('rust-structures')) Object.assign(fixtureExpected, { templates: !scenario.includes('-literal-'), topology: 'ring' });
  if (scenario.startsWith('cli-') && ['cli-all-1k', 'cli-common-10k', 'cli-expanded-10k'].includes(scenario)) Object.assign(fixtureExpected, { files: 10, fields: 20, variants: 1, coverage: 'sparse', topology: 'sparse' });
  if (/^(csharp|mono)-dim/u.test(scenario)) Object.assign(fixtureExpected, { case: 'dimensions', variants: scenario.includes('-dim10-') ? 10 : 1, density: scenario.includes('-dense-') ? 'dense' : 'sparse', topology: 'pairs', templates: !scenario.includes('-no-templates-') });
  const target = { id, scenario, metric, budget_source: reference, criteria, expected, fixture_expected: fixtureExpected, ...(note ? { note } : {}) };
  targets.push(target);
  return target;
}
add('rust-invalid-10k', 'rust-invalid-10000', 'validate_invalid', sources.invalid, [absolute('median_ms', 2000)], { records: 10000 });
add('rust-invalid-100k', 'rust-invalid-100000', 'validate_invalid', sources.invalid, [absolute('median_ms', 10000)], { records: 100000 });
add('rust-node-budget', 'rust-budget-20000', 'load_budget_rejected', sources.budget, [absolute('median_ms', 1000)], { records: 20000 });
for (const [metric, limit] of [['load_checks', 6000], ['load_no_checks', 3000]]) add(`rust-${metric}`, 'rust-main-100k', metric, sources.load, [absolute('median_ms', limit)], { records: 100000, fields: 20 });
add('rust-hot-scalar', 'rust-hot-100k', 'set_scalar', sources.hot, [absolute('median_ms', 35), absolute('p95_ms', 70, 'ms', { minimum_samples: 20 }), relative('median_ms', 25, { budget_source: sources.hotRelative })], { records: 100000 });
add('rust-grouped-1000', 'rust-main-100k', 'batch_grouped', sources.grouped, [absolute('median_ms', 100), relative('median_ms', 25)], { records: 100000, operations_per_sample: 1000 });
add('rust-interleaved-1000', 'rust-main-100k', 'batch_interleaved', sources.interleaved, [absolute('median_ms', 1500)], { records: 100000, operations_per_sample: 1000 });
for (const metric of ['insert_record', 'delete_record', 'rename_record', 'transfer_record']) {
  add(`rust-${metric}`, 'rust-structures-100k', metric, sources.members, [absolute('median_ms', 500)], { records: 100000 });
  add(`rust-literal-${metric}`, 'rust-structures-literal-100k', metric, sources.members, [absolute('median_ms', 500)], { records: 100000 }, '无模板对照单列；不能代替含模板的正式成员目标。');
}
for (const density of ['sparse', 'dense']) for (const metric of ['dimension_existing_set', 'dimension_existing_clear']) add(`rust-${density}-${metric}`, `rust-dim10-existing-${density}-10k`, metric, sources.dimension, [absolute('median_ms', density === 'sparse' ? 30 : 500)], { records: 10000 });
add('rust-projection', 'engine-model-10000', 'dimension_project_sparse_v1', sources.projection, [absolute('median_ms', 30)], { records: 10000, fields: 8 });
add('rust-projection-same', 'engine-model-10000', 'dimension_project_same_sparse_v1', sources.projection, [stable()], { records: 10000, fields: 8 });

const cliFamilies = [
  ['工程/Schema 检查', ['project_inspect', 'schema_inspect'], 50, 75],
  ['取值、列表、筛选排序、搜索、字段统计、结构/规则检查', ['data_get', 'data_list', 'filter_sort', 'data_search', 'stats_field', 'check_structure', 'check_rules'], 100, 500],
  ['反向引用查询', ['data_refs'], 200, 750],
  ['维度读取、缺失、差异、查询', ['dimension_get', 'dimension_missing', 'dimension_diff', 'dimension_query'], 150, 400],
  ['字段、数组/字典、记录成员、已有维度覆盖、源码写入', ['set_field', 'set_field_rules', 'array_insert', 'array_remove', 'array_move', 'dict_set', 'dict_remove', 'create_record', 'delete_record', 'rename_record', 'transfer_record', 'unset_field', 'dimension_set', 'dimension_clear', 'source_write'], 200, 2000],
  ['新维度变体', ['dimension_add_variant'], 300, 3000],
  ['按条件批量修改', ['set_where'], 500, 5000],
  ['非法值/源码拒绝、源码修复', ['reject_invalid_value', 'reject_invalid_source', 'repair_invalid_source'], 200, 1000],
  ['修订冲突拒绝', ['revision_conflict'], 150, 200],
];
// 原有八项仍绑定 common；其余万条基础操作固定等待 expanded 的正式证据。
const cliCommonMetrics = new Set(['data_get', 'check_rules', 'set_field', 'set_field_rules', 'set_noop', 'set_preview', 'batch_same_file', 'batch_cross_file']);
for (const [family, metrics, small, large] of cliFamilies) for (const [scale, limit] of [[1000, small], [10000, large]]) for (const metric of metrics) {
  const scenario = scale === 1000 ? 'cli-all-1k' : cliCommonMetrics.has(metric) ? 'cli-common-10k' : 'cli-expanded-10k';
  add(`cli-${scale}-${metric}`, scenario, metric, source(`| ${family} |`), [absolute('median_ms', limit), { ...stable(), only_if_baseline_within: limit }], { records: scale, fields: 20, ...(metric === 'set_where' && scale === 10000 ? { affected_records: 1000 } : {}) });
}
for (const metric of ['set_noop', 'set_preview']) add(`cli-10k-${metric}`, 'cli-common-10k', metric, sources.cli, [absolute('median_ms', 1000)], { records: 10000 });
for (const [metric, limit] of [['batch_same_file', 3000], ['batch_cross_file', 5000]]) add(`cli-10k-${metric}`, 'cli-common-10k', metric, sources.cliBatch, [absolute('median_ms', limit)], { records: 10000, affected_records: 1000 });

add('dotnet-scalar100k', 'csharp-scalar20-100k', 'load_text', sources.scalar, [absolute('median_ms', 3000), absolute('median_allocated_bytes', 1.5e9, 'bytes'), absolute('median_retained_delta_bytes', 400e6, 'bytes')], { records: 100000, fields: 20 });
add('dotnet-width100', 'csharp-width100-10k', 'load_text', sources.width, [absolute('median_ms', 1300), absolute('median_allocated_bytes', 750e6, 'bytes')], { records: 10000, fields: 100 });
add('dotnet-complex100k', 'csharp-complex-100k', 'load_text', sources.complex, [absolute('median_ms', 4000), absolute('median_allocated_bytes', 1.9e9, 'bytes'), absolute('median_retained_delta_bytes', 600e6, 'bytes')], { records: 100000 });
for (const host of ['csharp', 'mono']) {
  const allocation = host === 'csharp' ? 'median_allocated_bytes' : 'median_allocated_current_thread_bytes';
  for (const fixture of ['dim1-sparse-10k', 'dim10-sparse-10k', 'dim10-no-templates-10k']) {
    const criteria = [relative('median_ms', 60), relative(allocation, 60)];
    if (host === 'csharp' && fixture.startsWith('dim10')) criteria.push(absolute('median_ms', 3000));
    add(`${host}-${fixture}`, `${host}-${fixture}`, 'load_text', sources.dimensionLoad, criteria, { records: 10000 });
  }
  for (const metric of ['dimension_cached_for_batch', 'dimension_variants_batch', 'dimension_has_variant_batch']) {
    const criteria = [absolute(allocation, 0, 'bytes/sample', { statistic: 'maximum' })];
    if (host === 'csharp' && metric === 'dimension_cached_for_batch') criteria.push(absolute('median_ms', 1));
    add(`${host}-${metric}`, `${host}-dim1-sparse-10k`, metric, sources.cache, criteria, { records: 10000, operations_per_sample: 10000 }, host === 'mono' ? 'Mono 不支持分配计数时保留 null，不能判定零分配通过。' : null);
  }
}
add('dotnet-first-view', 'csharp-dim1-sparse-10k', 'dimension_first_all', sources.first, [absolute('median_ms', 150), relative('median_allocated_bytes', 50)], { records: 10000, operations_per_sample: 10000 });
for (const [fixture, limit] of [['scalar20-100k', 8000], ['complex-100k', 12000]]) add(`mono-${fixture}`, `mono-${fixture}`, 'load_text', sources.mono, [absolute('median_ms', limit), relative('median_ms', 40)], { records: 100000 });
add('il2cpp-real-player', 'il2cpp-scalar20-100k', 'load_text', sources.mono, [absolute('median_ms', 8000)], { records: 100000 }, 'IL2CPP 绝对预算需真实后端基线校准；没有 Player 证据即未验证。');

// 普通读取逐操作守住旧预算，不把族内较快操作替代较慢操作。
const readMetrics = new Set(['field_lookup', 'record_scan', 'references', 'dimension_lookup', 'refresh_same', 'table_lookup_batch', 'direct_property_batch', 'table_enumerate', 'dimension_fallback_cached_batch']);
const readKeys = new Set();
function addRead(scenario, metric) {
  const identity = `${scenario}:${metric}`;
  if (readKeys.has(identity)) return;
  readKeys.add(identity);
  add(`stability-${identity}`, scenario, metric, sources.stability, [stable()]);
}
// 固定计划的读取即使完全没有输出，也必须显示为证据不足。
for (const metric of ['field_lookup', 'record_scan', 'references', 'dimension_lookup', 'refresh_same']) addRead('rust-all-1k', metric);
for (const scenario of ['rust-common-10k', 'rust-hot-100k']) addRead(scenario, 'refresh_same');
for (const host of ['csharp', 'mono']) {
  for (const fixture of ['scalar20-1k', 'scalar20-100k', 'width8-10k', 'width100-10k', 'tables32-10k', 'complex-100k', 'templates-10k', 'ring-10k'])
    for (const metric of ['table_lookup_batch', 'direct_property_batch', 'table_enumerate']) addRead(`${host}-${fixture}`, metric);
  addRead(`${host}-dim1-sparse-10k`, 'dimension_fallback_cached_batch');
}
for (const observation of observations) if (readMetrics.has(observation.metric) && !observation.scenario.startsWith('cli-')) {
  addRead(observation.scenario, observation.metric);
}

// G5 独立于专项提速预算；健康加载、重载、恢复以及 Loader 拒绝路径不能只验普通查询。
// 固定矩阵先登记，即使整项没有输出也保留证据不足；额外观测再补充，避免只报告成功子集。
const loadMetrics = {
  rust: new Set(['load_checks', 'load_no_checks', 'error_recovery']),
  managed: new Set(['startup_load_text', 'load_text', 'read_files_and_load', 'reload_with_old_result_alive', 'load_after_invalid_input', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'reject_missing_reference']),
};
const loadKeys = new Set();
function addLoad(scenario, metric, expected = {}, fixture = {}) {
  const identity = `${scenario}:${metric}`;
  if (loadKeys.has(identity)) return;
  loadKeys.add(identity);
  const id = scenario === 'rust-recovery-10k' && metric === 'error_recovery' ? 'rust-recovery' : `stability-${identity}`;
  const target = add(id, scenario, metric, sources.loadStability, [stable()], expected,
    '加载/恢复/Loader 拒绝的同宿主、同夹具 P50 稳定性；与绝对预算及专项改善目标分别验收。进程峰值在独立表中验收。');
  Object.assign(target.fixture_expected, fixture);
  target.coverage = 'load_recovery_rejection_stability';
}
for (const [scenario, records, fields, files, variants, metrics] of [
  ['rust-all-1k', 1000, 20, 10, 1, ['load_checks', 'load_no_checks', 'error_recovery']],
  ['rust-common-10k', 10000, 20, 10, 1, ['load_checks', 'load_no_checks']],
  ['rust-main-100k', 100000, 20, 100, 0, ['load_checks', 'load_no_checks']],
  ['rust-width100-10k', 10000, 100, 10, 0, ['load_checks', 'load_no_checks']],
  ['rust-singlefile-10k', 10000, 20, 1, 0, ['load_checks']],
  ['rust-recovery-10k', 10000, 20, 10, 1, ['error_recovery']],
]) for (const metric of metrics) addLoad(scenario, metric, { records, fields }, { files, variants, topology: 'ring', templates: true });
const managedLoads = [
  ['scalar20-1k', 'scalar20', 1000, 20, 10, 'none', false, ['startup_load_text', 'load_text', 'read_files_and_load', 'reload_with_old_result_alive', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'load_after_invalid_input']],
  ['scalar20-100k', 'scalar20', 100000, 20, 100, 'none', false, ['load_text', 'read_files_and_load', 'reload_with_old_result_alive', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'load_after_invalid_input']],
  ['width8-10k', 'scalar8', 10000, 8, 100, 'none', false, ['load_text']],
  ['width100-10k', 'scalar100', 10000, 100, 100, 'none', false, ['load_text']],
  ['tables32-10k', 'tables32', 10000, 20, 100, 'none', false, ['load_text']],
  ['complex-100k', 'complex', 100000, 20, 100, 'pairs', false, ['load_text', 'reload_with_old_result_alive']],
  ['templates-10k', 'complex', 10000, 20, 100, 'pairs', true, ['load_text', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'reject_missing_reference', 'load_after_invalid_input']],
  ['ring-10k', 'complex', 10000, 20, 100, 'ring', true, ['load_text', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'reject_missing_reference', 'load_after_invalid_input']],
  ['dim1-sparse-10k', 'dimensions', 10000, 20, 100, 'pairs', true, ['load_text', 'reject_unknown_type_at_end', 'reject_syntax_at_end', 'reject_missing_reference', 'load_after_invalid_input']],
  ['dim10-sparse-10k', 'dimensions', 10000, 20, 100, 'pairs', true, ['load_text']],
  ['dim10-dense-10k', 'dimensions', 10000, 20, 100, 'pairs', true, ['load_text']],
  ['dim10-no-templates-10k', 'dimensions', 10000, 20, 100, 'pairs', false, ['load_text']],
];
for (const host of ['csharp', 'mono']) for (const [fixture, kind, records, fields, files, topology, templates, metrics] of managedLoads) {
  for (const metric of metrics) addLoad(`${host}-${fixture}`, metric, { records, fields }, { case: kind, files, topology, templates });
}
for (const row of observations) {
  const metrics = row.scenario.startsWith('rust-') ? loadMetrics.rust : /^(csharp|mono)-/u.test(row.scenario) ? loadMetrics.managed : null;
  if (metrics?.has(row.metric)) addLoad(row.scenario, row.metric);
}

function valueOf(observation, key, statistic = 'median') {
  if (key === 'sampled_process_peak_working_set_bytes') return observation.run_metadata?.[key];
  const detailKeys = { median_allocated_bytes: 'AllocatedBytes', median_allocated_current_thread_bytes: 'allocated_current_thread_bytes', median_retained_delta_bytes: observation.raw_metric?.samples_detail?.[0]?.RetainedDeltaBytes != null ? 'RetainedDeltaBytes' : 'retained_delta_bytes' };
  if (detailKeys[key]) {
    const details = observation.raw_metric?.samples_detail;
    // 每条分配/保留样本都必须可用，不能让缺失值参与排序并变成零。
    if (!details || details.length !== observation.samples || !details.every(sample => finite(sample[detailKeys[key]]))) return null;
    const values = details.map(sample => sample[detailKeys[key]]);
    return statistic === 'maximum' ? Math.max(...values) : median(values);
  }
  return observation[key];
}
function evaluate(target) {
  const all = observations.filter(row => row.scenario === target.scenario && row.metric === target.metric);
  const rejected = all.filter(row => row.problems.length || Object.entries(target.expected).some(([key, value]) => row[key] !== value) || Object.entries(target.fixture_expected ?? {}).some(([key, value]) => row.fixture_config[key] !== value));
  const valid = all.filter(row => !rejected.includes(row));
  const observedProcesses = [...runs.values()].flat().filter(run => run.spec?.scenario === target.scenario);
  const failedRuns = observedProcesses.filter(run => run.exit_code !== 0 || run.timed_out !== false);
  const criteria = target.criteria.map(criterion => {
    const measure = row => valueOf(row, criterion.key, criterion.statistic);
    const arms = Object.fromEntries(['baseline', 'optimized'].map(arm => [arm, valid.filter(row => row.arm === arm && finite(measure(row)) && row.samples >= (criterion.minimum_samples ?? 1))]));
    const pairs = arms.baseline.flatMap(before => {
      const after = arms.optimized.find(row => row.round === before.round && row.fixture_signature === before.fixture_signature && row.records === before.records && row.fields === before.fields && row.operations_per_sample === before.operations_per_sample);
      return after ? [{ round: before.round, baseline: measure(before), optimized: measure(after), ratio: measure(before) === 0 ? (measure(after) === 0 ? 1 : null) : measure(after) / measure(before), baseline_source: before.source, optimized_source: after.source }] : [];
    });
    const before = median(arms.baseline.map(measure));
    const after = criterion.statistic === 'maximum' && arms.optimized.length ? Math.max(...arms.optimized.map(measure)) : median(arms.optimized.map(measure));
    const pairedBefore = median(pairs.map(pair => pair.baseline)), pairedAfter = median(pairs.map(pair => pair.optimized));
    const ratio = pairedBefore === null || pairedAfter === null ? null : pairedBefore === 0 ? (pairedAfter === 0 ? 1 : null) : pairedAfter / pairedBefore;
    const applicable = criterion.only_if_baseline_within == null || arms.baseline.length < 3 || before === null || before <= criterion.only_if_baseline_within;
    const badArms = criterion.kind === 'relative' ? ['baseline', 'optimized'] : ['optimized'];
    const rejectedRelevant = rejected.filter(row => badArms.includes(row.arm));
    const failedRelevant = failedRuns.filter(run => badArms.includes(run.spec.arm));
    const enough = arms.optimized.length >= 3 && (criterion.kind !== 'relative' || pairs.length >= 3) && !rejectedRelevant.length && !failedRelevant.length;
    const observed = criterion.kind === 'relative' ? ratio : after;
    const status = !applicable ? 'not_applicable' : !enough || !finite(observed) ? 'insufficient_evidence' : observed <= criterion.limit ? 'passed' : 'not_met';
    const excluded = rejected.map(row => ({ source: row.source, arm: row.arm, round: row.round, value: finite(measure(row)) ? measure(row) : null, metric_correctness: row.raw_metric?.correctness ?? null, problems: row.problems.length ? row.problems : ['target_fixture_mismatch'] }));
    return { ...criterion, status, observed, baseline: before, optimized: after, optimized_processes: arms.optimized.length, baseline_processes: arms.baseline.length, paired_rounds: pairs.length, pairs, excluded_observations: excluded, rejected_sources: rejectedRelevant.map(row => ({ source: row.source, problems: row.problems.length ? row.problems : ['target_fixture_mismatch'] })), failed_processes: failedRelevant.map(run => ({ source: run.source, exit_code: run.exit_code, timed_out: run.timed_out })) };
  });
  const overall = criteria.some(item => item.status === 'not_met') ? 'not_met' : criteria.some(item => item.status === 'insufficient_evidence') ? 'insufficient_evidence' : 'passed';
  const reference = (comparison.comparisons ?? []).filter(row => row.scenario === target.scenario && row.metric === target.metric);
  return { ...target, status: overall, criteria, comparison_reference: reference.map(row => ({ baseline_ms: row.baseline_ms, optimized_ms: row.optimized_ms, paired_rounds: row.paired_rounds })), evidence: all.map(row => ({ source: row.source, arm: row.arm, round: row.round, samples: row.samples, median_ms: row.median_ms, p95_ms: row.p95_ms, operations_per_sample: row.operations_per_sample, affected_records: row.affected_records, metric_correctness: row.raw_metric?.correctness ?? null, problems: row.problems })) };
}

const evaluated = targets.map(evaluate);
// 进程峰值包括夹具、预热与正确性检查；CLI 驱动进程不代表子 CLI，交给独立内存报告。
const peakTargets = [];
const scenarios = [...new Set([...observations.map(row => row.scenario), ...targets.filter(target => target.coverage).map(target => target.scenario)])].filter(scenario => !scenario.startsWith('cli-'));
for (const scenario of scenarios) {
  const rows = observations.filter(row => row.scenario === scenario);
  // model_build 在同进程含三种字段宽度，峰值选择每轮唯一的指标作完成证据。
  const metric = rows.find(row => rows.filter(other => other.metric === row.metric && other.arm === row.arm && other.round === row.round).length === 1)?.metric
    ?? targets.find(target => target.scenario === scenario && target.coverage)?.metric;
  if (!metric) continue;
  peakTargets.push(evaluate({ id: `peak-${scenario}`, scenario, metric, budget_source: sources.stability, criteria: [stable('sampled_process_peak_working_set_bytes')], expected: {}, note: '整个基准进程的采样峰值，含夹具/预热/断言；不等同于数据模型保留堆。' }));
}
const counts = items => Object.fromEntries(['passed', 'not_met', 'insufficient_evidence'].map(status => [status, items.filter(item => item.status === status).length]));
const loadTargets = evaluated.filter(target => target.coverage === 'load_recovery_rejection_stability');
const loadCoverage = {
  budget_source: sources.loadStability,
  scope: 'Fixed Rust and .NET/Mono load/reload/recovery/Loader-rejection matrix, plus matching observed metrics. CLI rejection/repair retain their existing conditional stability criteria. Rust template-cascade validation and structural-budget rejection retain their dedicated P0 targets.',
  target_count: loadTargets.length, counts: counts(loadTargets),
  by_host: Object.fromEntries(['rust', 'csharp', 'mono'].map(host => [host, counts(loadTargets.filter(target => target.scenario.startsWith(`${host}-`)))])),
  missing_metric_evidence: loadTargets.filter(target => !target.evidence.length).map(target => ({ id: target.id, scenario: target.scenario, metric: target.metric })),
  incomplete_pair_evidence: loadTargets.filter(target => target.criteria.some(criterion => criterion.paired_rounds < 3)).map(target => ({ id: target.id, paired_rounds: target.criteria[0].paired_rounds, failed_processes: target.criteria[0].failed_processes })),
};
const report = {
  method: 'Absolute targets require at least three independent successful optimized processes. Relative targets require at least three same-round, same-fixture successful pairs. The statistic is the arithmetic median of per-process medians; ratios use only valid pairs. P95 uses the median of observed per-process P95 values and requires at least 20 raw samples in each contributing process. Zero-allocation targets require every raw sample in every contributing optimized process to be zero. Duplicate rounds, failed/censored runs, incomplete managed output, missing data and null counters cannot pass. MB/GB are decimal bytes. Descriptive sample results are not confidence intervals.',
  inputs: [{ path: summaryPath, sha256: hash(summaryPath) }, { path: comparisonPath, sha256: hash(comparisonPath) }, { path: budgetPath, sha256: hash(budgetPath) }],
  counts: counts(evaluated), peak_counts: counts(peakTargets), load_stability_coverage: loadCoverage, targets: evaluated, process_peak_targets: peakTargets,
  limitations: ['CLI direct child-process memory uses its separate report.', 'Missing scenarios remain insufficient evidence; successful subset results do not imply complete target delivery.', 'Performance criteria do not replace semantic and release gates.', 'Mono allocation counters may be unsupported; null is never zero.', 'Literal-only membership results do not replace the template fixture target.', 'IL2CPP remains unverified without an actual player baseline.'],
};
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'target-status.json'), JSON.stringify(report, null, 2) + '\n');
const statusText = { passed: '达标', not_met: '未达标', insufficient_evidence: '证据不足', not_applicable: '不适用' };
const format = value => value == null ? '—' : Number.isInteger(value) ? String(value) : value.toFixed(4);
const coverageMarkdown = `加载/恢复/Loader 拒绝稳定性覆盖 ${loadCoverage.target_count} 项：达标 ${loadCoverage.counts.passed}，未达标 ${loadCoverage.counts.not_met}，证据不足 ${loadCoverage.counts.insufficient_evidence}；完全缺少指标证据 ${loadCoverage.missing_metric_evidence.length} 项，不足三对有效进程 ${loadCoverage.incomplete_pair_evidence.length} 项。固定矩阵含 .NET/Mono ring、文本/文件加载、首次加载、保留旧结果重载、错误后恢复及类型/语法/引用拒绝。CLI 拒绝/修复沿用原条件回归预算，Rust 模板级联及节点预算拒绝沿用 P0 专项。\n\n`;
const table = items => '| 目标 / 操作 | 条件 | 验收观测值 | 排除样本的优化观测值 | 优化进程 / 配对轮数 | 判定 |\n| --- | --- | ---: | ---: | ---: | --- |\n' + items.flatMap(target => target.criteria.map(criterion => {
  const excluded = criterion.excluded_observations.filter(row => row.arm === 'optimized' && finite(row.value));
  return `| ${target.id} / ${target.metric} | ${criterion.key} ≤ ${format(criterion.limit)} ${criterion.unit} | ${format(criterion.observed)} | ${format(median(excluded.map(row => row.value)))} | ${criterion.optimized_processes} / ${criterion.paired_rounds} | ${statusText[criterion.status]} |`;
})).join('\n');
const markdown = '# 优化目标验收\n\n来源为原始优化预算；所有目标保持独立判定。绝对目标至少 3 个成功优化进程，相对目标至少 3 对同轮同夹具进程。常规值为进程 P50 的算术中位数；P95 为各进程观察 P95 的中位数，每个进程至少 20 个原始样本。零分配要求所有优化原始样本均为零。失败、超时、缺失与不支持的分配计数不能判通过。MB/GB 按十进制字节。\n\n排除样本的观测值仍保留，包含部分失败 CLI 进程已完成的操作；该列按条件原始单位给出优化侧中位数，不是相对比例，不参与验收。具体正确性状态和排除原因见 JSON。\n\n' + `目标：达标 ${report.counts.passed}，未达标 ${report.counts.not_met}，证据不足 ${report.counts.insufficient_evidence}。这是逐项证据状态，不表示全部优化或功能门禁完成。\n\n` + coverageMarkdown + table(evaluated) + '\n\n## 进程峰值稳定性\n\n峰值包含夹具、预热和断言。CLI 子进程内存单独报告。\n\n' + table(peakTargets) + '\n\n完整阈值原文、来源行号、每轮值、原始证据路径和排除理由见 `target-status.json`。Mono 分配 null 保持未验证；IL2CPP 缺失真实 Player 数据保持未验证。\n';
fs.writeFileSync(path.join(output, 'target-status.md'), markdown);
console.log(JSON.stringify({ output, targets: report.counts, load_stability: loadCoverage.counts, process_peaks: report.peak_counts }));
