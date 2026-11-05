// 只聚合已归档的直接 CLI 进程；不启动 CLI、不生成夹具，也不补造缺失的峰值。
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const usage = 'Usage: node report-cli-memory.mjs <evidence-directory> [--output <report-directory>] [--baseline-sha256 <hash>] [--optimized-sha256 <hash>]';
if (!args[0] || args[0].startsWith('--') || args.length % 2 === 0) throw new Error(usage);
const options = new Map();
for (let index = 1; index < args.length; index += 2) {
  if (!['--output', '--baseline-sha256', '--optimized-sha256'].includes(args[index]) || !args[index + 1] || options.has(args[index])) throw new Error(usage);
  options.set(args[index], args[index + 1]);
}
const directory = path.resolve(args[0]);
const output = path.resolve(options.get('--output') ?? directory);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const validHash = value => typeof value === 'string' && /^[a-f0-9]{64}$/iu.test(value);
const normalizeHash = value => validHash(value) ? value.toLowerCase() : null;
const finite = value => typeof value === 'number' && Number.isFinite(value);
const median = values => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const range = values => values.length ? { min: Math.min(...values), max: Math.max(...values) } : { min: null, max: null };
const canonical = value => value && typeof value === 'object' ? Array.isArray(value) ? value.map(canonical)
  : Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
const same = (left, right) => JSON.stringify(canonical(left)) === JSON.stringify(canonical(right));
const pathKey = value => typeof value === 'string' ? value.replaceAll('\\', '/').replace(/\/+$/u, '').toLowerCase() : null;
const joinRecordedPath = (root, name) => /^[a-z]:[\\/]/iu.test(root) || root.startsWith('\\\\') ? path.win32.join(root, name) : path.join(root, name);
const expectedHashes = Object.fromEntries(['baseline', 'optimized'].map(arm => {
  const value = options.get(`--${arm}-sha256`);
  if (value != null && !validHash(value)) throw new Error(`Invalid ${arm} SHA256`);
  return [arm, normalizeHash(value)];
}));
const inputs = new Map();
const issues = [];
const note = (scope, code, detail = null) => issues.push({ scope, code, ...(detail == null ? {} : { detail }) });
function read(relative, problems, optional = false) {
  const file = path.resolve(directory, relative);
  if (!fs.existsSync(file)) {
    if (!optional) problems.push(`missing:${relative}`);
    return null;
  }
  try {
    const content = fs.readFileSync(file);
    inputs.set(relative, { path: relative.replaceAll('\\', '/'), sha256: sha(content) });
    const value = JSON.parse(content.toString('utf8').replace(/^\uFEFF/u, ''));
    // 合法 JSON 的 null/标量仍不是证据对象，不能因后续真假分支跳过验收。
    if (value == null || typeof value !== 'object' || Array.isArray(value)) {
      problems.push(`invalid_evidence_object:${relative}`);
      return null;
    }
    return value;
  } catch (error) {
    problems.push(`unreadable:${relative}`);
    note(relative, 'unreadable_json', error.message);
    return null;
  }
}
const budgetPath = path.join(repo, 'docs/archive/benchmarks/non-editor-2026-10-28/README.md');
const budgetText = fs.readFileSync(budgetPath, 'utf8');
function budget(phrase) {
  const lines = budgetText.split(/\r?\n/u), line = lines.findIndex(value => value.includes(phrase));
  if (line < 0) throw new Error(`Budget source missing: ${phrase}`);
  return { path: path.relative(repo, budgetPath).replaceAll('\\', '/'), line: line + 1, text: lines[line] };
}
const budgets = { absolute: budget('| P2：CLI 进程内存'), relative: budget('先要求各自口径的峰值不回归超过 5%') };
const shapes = [
  { records: 10000, files: 10, cases: ['set', 'batch'], limit_bytes: 500e6 },
  { records: 100000, files: 100, cases: ['get', 'check'], limit_bytes: 1.2e9 },
];
const observations = [];
const groups = [];
const commands = (name, records, root) => {
  const operations = {
    get: ['data', 'get', '--type', 'Row', '--id', 'r000000'],
    check: ['check', '--rules'],
    set: ['data', 'set', '--type', 'Row', '--id', 'r000000', '--path', 'value', '--value-cfd', String(records + 2), '--rules'],
    batch: ['apply', '--input', joinRecordedPath(root, 'memory-batch.json'), '--rules'],
  };
  return [...operations[name], '--project', root, '--json'];
};
const canonicalArguments = values => Array.isArray(values) ? values.map((value, index) => ['--project', '--input'].includes(values[index - 1]) ? pathKey(value) : value) : null;
const checkPassed = value => value?.structure === 'passed' && value?.rules === 'passed' && value?.complete === true;

// 每轮每臂固定四个被采样命令。复读和规则复核由 verification 证明，不混入这 24 个峰值。
for (const round of [1, 2, 3]) for (const arm of ['baseline', 'optimized']) for (const shape of shapes) {
  const relative = path.join(`r${round}-${arm}`, String(shape.records));
  const prefix = `cli-memory-${shape.records}`;
  const problems = [];
  const fixture = read(path.join(relative, `${prefix}-fixture.json`), problems);
  const verification = read(path.join(relative, `${prefix}-verification.json`), problems);
  const expectedConfig = { records: shape.records, files: shape.files, fields: 20, batch: 1000, variants: 1, coverage: 'sparse', topology: 'sparse' };
  if (fixture && (fixture.protocol_version !== 1 || fixture.suite !== 'cli_matrix_fixture' || fixture.cli_executed !== false
      || typeof fixture.root !== 'string' || !fixture.root || !Number.isSafeInteger(fixture.source_bytes) || fixture.source_bytes <= 0
      || !validHash(fixture.fixture_sha256) || Object.entries(expectedConfig).some(([key, value]) => fixture.config?.[key] !== value))) problems.push('fixture_contract_mismatch');
  if (verification && (verification.status !== 'passed' || !same(verification.selected_cases, shape.cases)
      || verification.batch_records !== (shape.cases.includes('batch') ? 1000 : 0))) problems.push('verification_contract_mismatch');
  if (verification && (!validHash(verification.cli_sha256) || !validHash(verification.fixture_sha256)
      || normalizeHash(verification.fixture_sha256) !== normalizeHash(fixture?.fixture_sha256))) problems.push('verification_hash_mismatch');
  const group = { round, arm, records: shape.records, path: relative.replaceAll('\\', '/'), verification_status: verification?.status ?? null,
    fixture_sha256: normalizeHash(fixture?.fixture_sha256), source_bytes: fixture?.source_bytes ?? null, config: fixture?.config ?? null, problems };
  groups.push(group);
  const found = fs.existsSync(path.join(directory, relative)) ? fs.readdirSync(path.join(directory, relative)).filter(name => /^cli-memory-.*\.meta\.json$/u.test(name)) : [];
  for (const name of found) if (!shape.cases.some(operation => name === `${prefix}-${operation}.meta.json`)) {
    problems.push(`unexpected_process:${name}`);
  }
  for (const operation of shape.cases) {
    const localProblems = [...problems];
    const id = `${prefix}-${operation}`, stem = path.join(relative, id);
    const metadata = read(`${stem}.meta.json`, localProblems);
    const sidecar = read(`${stem}.spec.json`, localProblems, true);
    const response = read(`${stem}.stdout.ndjson`, localProblems);
    const spec = metadata?.spec;
    if (metadata) {
      if (metadata.id !== id || spec?.id !== id) localProblems.push('process_identity_mismatch');
      if (metadata.exit_code !== 0 || metadata.timed_out !== false) localProblems.push('failed_or_censored_process');
      if (!Number.isSafeInteger(metadata.process_id) || metadata.process_id <= 0 || !Number.isFinite(Date.parse(metadata.started_at))
          || !finite(metadata.wall_seconds) || metadata.wall_seconds < 0) localProblems.push('invalid_process_interval');
      if (!Number.isSafeInteger(metadata.sampled_process_peak_working_set_bytes) || metadata.sampled_process_peak_working_set_bytes <= 0
          || !Number.isSafeInteger(metadata.process_memory_samples) || metadata.process_memory_samples <= 0) localProblems.push('missing_peak_measurement');
      if (!Number.isSafeInteger(metadata.process_memory_sample_errors) || metadata.process_memory_sample_errors < 0) localProblems.push('invalid_memory_error_count');
      if (sidecar && !same(spec, sidecar)) localProblems.push('sidecar_spec_mismatch');
      if (!spec || typeof spec.executable !== 'string' || pathKey(spec.executable)?.split('/').at(-1) !== 'coflow.exe'
          || spec.environment?.COFLOW_LOG !== '0' || typeof fixture?.root !== 'string' || !fixture.root
          || !same(canonicalArguments(spec.arguments), canonicalArguments(commands(operation, shape.records, fixture.root)))) localProblems.push('direct_cli_spec_mismatch');
      if (typeof spec?.memory_scope !== 'string' || !spec.memory_scope.startsWith('Direct coflow.exe process for one command,')
          || metadata.memory_scope !== spec.memory_scope) localProblems.push('direct_cli_memory_scope_mismatch');
      const captured = Array.isArray(metadata.input_hashes) ? metadata.input_hashes.filter(item => pathKey(item?.path) === pathKey(spec?.executable)) : [];
      if (!validHash(spec?.cli_sha256) || captured.length !== 1 || normalizeHash(captured[0]?.sha256) !== normalizeHash(spec?.cli_sha256)
          || normalizeHash(spec?.cli_sha256) !== normalizeHash(verification?.cli_sha256)
          || (expectedHashes[arm] != null && normalizeHash(spec?.cli_sha256) !== expectedHashes[arm])) localProblems.push('cli_hash_mismatch');
      if (!validHash(spec?.fixture_sha256) || normalizeHash(spec?.fixture_sha256) !== normalizeHash(fixture?.fixture_sha256)) localProblems.push('spec_fixture_hash_mismatch');
    }
    if (response) {
      if (response.protocol_version !== 1 || response.status !== 'success' || !Array.isArray(response.diagnostics)
          || response.diagnostics.some(item => item?.severity === 'error')) localProblems.push('command_response_failed');
      const result = response.result;
      if (operation === 'get' && (result?.id !== 'r000000' || result.fields?.value?.kind !== 'int'
          || !['string', 'number'].includes(typeof result.fields.value.value) || String(result.fields.value.value) !== '0')) localProblems.push('get_result_mismatch');
      if (operation === 'check' && !checkPassed(result)) localProblems.push('check_result_mismatch');
      if (['set', 'batch'].includes(operation) && (result?.written !== true || result.changed !== true || !checkPassed(result.validation))) localProblems.push('write_result_mismatch');
      if (operation === 'batch' && result?.changed_records !== 1000) localProblems.push('batch_record_count_mismatch');
    }
    observations.push({ round, arm, records: shape.records, operation, source: `${stem}.meta.json`.replaceAll('\\', '/'),
      metadata_present: metadata != null, process_id: metadata?.process_id ?? null, started_at: metadata?.started_at ?? null,
      wall_seconds: metadata?.wall_seconds ?? null, peak_bytes: finite(metadata?.sampled_process_peak_working_set_bytes) ? metadata.sampled_process_peak_working_set_bytes : null,
      process_memory_samples: metadata?.process_memory_samples ?? null, process_memory_sample_errors: metadata?.process_memory_sample_errors ?? null,
      cli_sha256: normalizeHash(spec?.cli_sha256), fixture_sha256: normalizeHash(fixture?.fixture_sha256),
      source_bytes: fixture?.source_bytes ?? null, verification_status: verification?.status ?? null, problems: localProblems });
  }
}

// 相同规模必须使用同一初始输入；同一臂不能混用多个 CLI 产物。
for (const arm of ['baseline', 'optimized']) {
  const rows = observations.filter(row => row.arm === arm), hashes = new Set(rows.map(row => row.cli_sha256).filter(Boolean));
  if (hashes.size > 1) for (const row of rows) row.problems.push('mixed_cli_artifacts_in_arm');
}
for (const shape of shapes) {
  const rows = observations.filter(row => row.records === shape.records);
  if (new Set(rows.filter(row => row.fixture_sha256).map(row => `${row.fixture_sha256}:${row.source_bytes}`)).size > 1) {
    for (const row of rows) row.problems.push('fixture_changed_between_processes');
  }
}
const intervals = observations.filter(row => row.metadata_present && Number.isFinite(Date.parse(row.started_at)) && finite(row.wall_seconds))
  .map(row => ({ row, start: Date.parse(row.started_at), end: Date.parse(row.started_at) + row.wall_seconds * 1000 })).sort((a, b) => a.start - b.start);
for (let index = 1; index < intervals.length; index++) {
  const previous = intervals[index - 1], current = intervals[index];
  if (previous.end > current.start) {
    previous.row.problems.push('sampled_cli_processes_overlap');
    current.row.problems.push('sampled_cli_processes_overlap');
  }
  if (previous.row.round > current.row.round) {
    previous.row.problems.push('round_order_mismatch');
    current.row.problems.push('round_order_mismatch');
  }
}
const order = [];
for (const round of [1, 2, 3]) {
  const expected = round === 2 ? ['optimized', 'baseline'] : ['baseline', 'optimized'];
  const first = intervals.filter(item => item.row.round === round && item.row.arm === expected[0]);
  const second = intervals.filter(item => item.row.round === round && item.row.arm === expected[1]);
  const complete = first.length === 4 && second.length === 4;
  const correct = first.length && second.length ? Math.max(...first.map(item => item.end)) <= Math.min(...second.map(item => item.start)) : null;
  if (correct === false) for (const item of [...first, ...second]) item.row.problems.push('arm_order_mismatch');
  order.push({ round, expected, complete, observed_order_matches: correct });
}
for (const row of observations) {
  if (row.metadata_present && observations.some(other => other !== row && other.metadata_present && other.process_id === row.process_id && other.started_at === row.started_at)) row.problems.push('duplicate_process_evidence');
  row.problems = [...new Set(row.problems)];
  row.valid = row.problems.length === 0;
  for (const problem of row.problems) note(row.source, problem);
}
const results = shapes.flatMap(shape => shape.cases.map(operation => {
  const rows = observations.filter(row => row.records === shape.records && row.operation === operation);
  const pairs = [1, 2, 3].map(round => {
    const before = rows.find(row => row.round === round && row.arm === 'baseline');
    const after = rows.find(row => row.round === round && row.arm === 'optimized');
    const valid = before.valid && after.valid && before.fixture_sha256 === after.fixture_sha256
      && finite(before.peak_bytes) && before.peak_bytes > 0 && finite(after.peak_bytes) && after.peak_bytes > 0;
    return { round, valid, baseline_bytes: before.peak_bytes, optimized_bytes: after.peak_bytes,
      ratio: valid ? after.peak_bytes / before.peak_bytes : null, baseline_source: before.source, optimized_source: after.source,
      problems: valid ? [] : [...before.problems.map(problem => `baseline:${problem}`), ...after.problems.map(problem => `optimized:${problem}`)] };
  });
  const accepted = pairs.filter(pair => pair.valid), before = accepted.map(pair => pair.baseline_bytes), after = accepted.map(pair => pair.optimized_bytes);
  const baseline = median(before), optimized = median(after), ratio = baseline == null ? null : optimized / baseline;
  const enough = accepted.length === 3;
  const criteria = [
    { kind: 'absolute', unit: 'bytes', limit: shape.limit_bytes, observed: optimized, budget_source: budgets.absolute },
    { kind: 'relative', unit: 'optimized/baseline', limit: 1.05, observed: ratio, budget_source: budgets.relative },
  ].map(criterion => ({ ...criterion, status: !enough || !finite(criterion.observed) ? 'insufficient_evidence' : criterion.observed <= criterion.limit ? 'passed' : 'not_met' }));
  return { id: `cli-memory-${shape.records}-${operation}`, records: shape.records, operation, paired_rounds: accepted.length,
    baseline_median_bytes: baseline, optimized_median_bytes: optimized, baseline_range_bytes: range(before), optimized_range_bytes: range(after),
    paired_peak_ratio: ratio, criteria, pairs,
    status: criteria.some(item => item.status === 'not_met') ? 'not_met' : criteria.every(item => item.status === 'passed') ? 'passed' : 'insufficient_evidence' };
}));
const counts = Object.fromEntries(['passed', 'not_met', 'insufficient_evidence'].map(status => [status, results.filter(result => result.status === status).length]));
const report = {
  method: 'Three serial independent same-round fixture-matched pairs per operation, AB/BA/AB by round. Each datum is one direct coflow.exe OS cumulative peak, sampled approximately every 100ms with best-effort startup/exit observations. Absolute targets use the arithmetic median of optimized peaks from the three valid pairs; relative targets use the ratio of optimized/baseline medians from those same pairs. Per-round values and full ranges are retained. Any missing/invalid/null-count process prevents that operation from passing. Decimal MB/GB. No latency distribution or allocation/retained-heap inference.',
  evidence_directory: directory, expected_sampled_processes: 24, observed_process_metadata: observations.filter(row => row.metadata_present).length,
  valid_sampled_processes: observations.filter(row => row.valid).length, expected_fixture_verifications: 12,
  passed_fixture_verifications: groups.filter(group => group.verification_status === 'passed' && !group.problems.length).length,
  cli_artifacts: Object.fromEntries(['baseline', 'optimized'].map(arm => [arm, { expected_sha256: expectedHashes[arm], observed_sha256: [...new Set(observations.filter(row => row.arm === arm).map(row => row.cli_sha256).filter(Boolean))] }])),
  round_order: order, counts, results, observations, fixture_verifications: groups, issues,
  inputs: [...inputs.values()], budget_input: { path: budgetPath, sha256: sha(budgetText) },
  limitations: ['Only sampled command processes count toward 24; fixture creation and subsequent verification commands are excluded.',
    'Last-interval process peaks can be missed; null or nonpositive measurements never become zero.',
    'CLI hashes are checked against archived spec, measured executable input hash and verification; optional expected hashes bind the report to independently frozen artifacts.',
    'Fixture hashes are cross-checked across the archived fixture, spec and verification; temporary source directories are removed by the measurement tool, so this report cannot rehash their historical bytes.',
    'Filesystem caches are uncontrolled. Three peaks describe these runs and are not confidence intervals or permanent memory guarantees.'],
};
const statusText = { passed: '达标', not_met: '未达标', insufficient_evidence: '证据不足' };
const mb = value => finite(value) ? (value / 1e6).toFixed(2) : '—';
const ratioText = value => finite(value) ? value.toFixed(4) : '—';
const rows = results.map(result => `| ${result.id} | ${mb(result.baseline_median_bytes)} | ${mb(result.optimized_median_bytes)} | ${mb(result.baseline_range_bytes.min)}–${mb(result.baseline_range_bytes.max)} | ${mb(result.optimized_range_bytes.min)}–${mb(result.optimized_range_bytes.max)} | ${ratioText(result.paired_peak_ratio)} | ${mb(result.criteria[0].limit)} | ${result.paired_rounds}/3 | ${statusText[result.status]} |`).join('\n');
const pairs = results.flatMap(result => result.pairs.map(pair => `| ${result.id} | ${pair.round} | ${mb(pair.baseline_bytes)} | ${mb(pair.optimized_bytes)} | ${ratioText(pair.ratio)} | ${pair.valid ? '有效' : '排除，原因见 JSON'} |`)).join('\n');
const markdown = '# 独立 CLI 进程内存验收\n\n只分析已有测量。每项需要三对同轮、同夹具、验证通过的独立进程，轮次顺序为 AB/BA/AB。绝对值取三对中优化侧峰值的算术中位数，相对值取同三对两侧中位数之比，要求 ≤1.05。保留每轮值及范围；缺失、超时、校验失败或空计数不能通过。MB 按十进制字节。\n\n'
  + `预期 24 个被采样 coflow.exe 进程，已见元数据 ${report.observed_process_metadata}，有效 ${report.valid_sampled_processes}；预期 12 份夹具复核，完整通过 ${report.passed_fixture_verifications}。四项目标：达标 ${counts.passed}，未达标 ${counts.not_met}，证据不足 ${counts.insufficient_evidence}。\n\n`
  + '| 操作 | 基线中位数 MB | 优化中位数 MB | 基线范围 MB | 优化范围 MB | 峰值比 | 绝对上限 MB | 有效配对 | 判定 |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n' + rows
  + '\n\n| 操作 | 轮次 | 基线 MB | 优化 MB | 峰值比 | 证据 |\n| --- | ---: | ---: | ---: | ---: | --- |\n' + pairs
  + '\n\n范围仅汇总有效配对；每轮表保留被排除的实测峰值，不能据其通过验收。进程峰值由 OS 累计值约每 100 ms 采样，最后区间可能缺测；它不是托管分配量或模型保留堆，也不是时延分布。Node/PowerShell、夹具生成和后续复核不计入被采样进程。CLI 摘要、夹具摘要、每轮 verification 状态、原始证据与排除原因见 `cli-memory-report.json`。\n';
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'cli-memory-report.json'), JSON.stringify(report, null, 2) + '\n');
fs.writeFileSync(path.join(output, 'cli-memory-report.md'), markdown);
console.log(JSON.stringify({ output, expected_processes: 24, observed_processes: report.observed_process_metadata, valid_processes: report.valid_sampled_processes, targets: counts, issues: issues.length }));
