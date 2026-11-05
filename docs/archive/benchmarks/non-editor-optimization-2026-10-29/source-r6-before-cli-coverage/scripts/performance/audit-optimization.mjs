// 只读审计已归档的成对测量；未退出进程不纳入成功或比较结果。
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

const directory = path.resolve(process.argv[2]);
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const stable = value => value && typeof value === 'object'
  ? Array.isArray(value) ? value.map(stable) : Object.fromEntries(Object.keys(value).sort().map(key => [key, stable(value[key])]))
  : value;
const same = (left, right) => assert.deepEqual(stable(left), stable(right));
const issues = [];
const verify = (scope, work) => { try { work(); return true; } catch (error) { issues.push({ scope, error: error.message }); return false; } };
const manifests = fs.readdirSync(directory).filter(file => /^plan-.*\.json$/u.test(file)).map(file => ({ file, ...read(path.join(directory, file)) }));
assert.ok(manifests.length, 'No suite plan found');
const specifications = new Map();
const payloads = new Map();
const coordinates = new Map();
for (const manifest of manifests) {
  for (const item of manifest.payloads) {
    const key = item.path.toLowerCase();
    if (payloads.has(key)) verify(manifest.file, () => same(payloads.get(key), item.sha256.toLowerCase()));
    payloads.set(key, item.sha256.toLowerCase());
  }
  for (const spec of manifest.plans) {
    if (specifications.has(spec.id)) verify(spec.id, () => same(specifications.get(spec.id), spec));
    const coordinate = JSON.stringify([spec.scenario, spec.round, spec.arm]);
    if (coordinates.has(coordinate)) verify(spec.id, () => same(coordinates.get(coordinate), spec.id));
    coordinates.set(coordinate, spec.id);
    specifications.set(spec.id, spec);
  }
}
function options(spec) {
  const result = {};
  for (let i = 0; i < spec.arguments.length; i++) {
    const key = spec.arguments[i];
    if (key.startsWith('--')) result[key.slice(2)] = spec.arguments[++i];
  }
  delete result.cli;
  delete result.output;
  return result;
}
function expectedMetrics(spec) {
  if (spec.group === 'rust') return spec.environment.MATRIX_CASES?.split(',') ?? [
    'load_checks', 'load_no_checks', 'field_lookup', 'record_scan', 'references', 'validate_same', 'validate_changed',
    'validate_invalid', 'refresh_same', 'error_recovery', 'set_scalar', 'set_dependency', 'set_noop', 'unset_field',
    'batch_grouped', 'batch_interleaved', 'array_element', 'array_replace', 'dict_element', 'dict_replace', 'nested_field',
    'option_field', 'insert_record', 'delete_record', 'rename_record', 'swap_records', 'transfer_record', 'dimension_set',
    'dimension_add', 'dimension_clear', 'dimension_lookup', 'external_conflict'];
  if (spec.group === 'model' && spec.environment.ENGINE_MODE === 'model') {
    const metrics = [8, 32, 128].flatMap(fields => ['model_build', `model_clone_10000_f${fields}`]);
    for (const variants of [1, 10]) for (const coverage of ['sparse', 'dense']) {
      metrics.push(`dimension_build_${coverage}_v${variants}`, `dimension_project_${coverage}_v${variants}`, `dimension_project_same_${coverage}_v${variants}`);
    }
    for (const count of [100, 1000, 5000]) for (const broken of [false, true]) {
      if (!broken || count <= Number(spec.environment.ENGINE_BROKEN_RECORDS_MAX ?? 1000)) metrics.push(`parse_${count}_broken_${broken}`);
    }
    return metrics;
  }
  const args = options(spec);
  if (spec.group === 'cli') return args.case === 'all' ? [
    'project_inspect', 'schema_inspect', 'data_get', 'data_list', 'filter_sort', 'data_search', 'data_refs',
    'check_structure', 'check_rules', 'stats_field', 'dimension_get', 'dimension_missing', 'dimension_diff', 'dimension_query',
    'set_field', 'set_field_rules', 'set_noop', 'set_preview', 'batch_same_file', 'batch_cross_file', 'set_where',
    'array_insert', 'array_remove', 'array_move', 'dict_set', 'dict_remove', 'create_record', 'delete_record',
    'rename_record', 'transfer_record', 'unset_field', 'dimension_set', 'dimension_add_variant', 'dimension_clear',
    'source_write', 'reject_invalid_value', 'reject_invalid_source', 'repair_invalid_source', 'revision_conflict'] : args.case.split(',');
  if (!['csharp', 'mono'].includes(spec.group)) return null;
  const metrics = [];
  const groups = args.operations.split(',');
  if (groups.includes('startup')) metrics.push('startup_load_text');
  if (groups.includes('load')) metrics.push('load_text');
  if (groups.includes('io')) metrics.push('read_files_and_load');
  if (groups.includes('reload')) metrics.push('reload_with_old_result_alive');
  if (groups.includes('queries')) metrics.push('table_lookup_batch', 'direct_property_batch', 'table_enumerate');
  if (groups.includes('dimensions')) metrics.push('dimension_first_record', 'dimension_first_all', 'dimension_cached_for_batch',
    'dimension_combined_first_record', 'dimension_fallback_first_record', 'dimension_fallback_cached_batch', 'dimension_variants_batch', 'dimension_has_variant_batch');
  if (groups.includes('error')) {
    metrics.push('reject_unknown_type_at_end', 'reject_syntax_at_end');
    if (['complex', 'dimensions'].includes(args.case)) metrics.push('reject_missing_reference');
    metrics.push('load_after_invalid_input');
  }
  return metrics;
}
const runs = [];
const completed = new Map();
const operationFailures = [];
// 历史驱动用明确的 timeout 异常和耗时记录超时；不把普通错误文字猜成超时。
function cliFailure(row, timeoutMs) {
  const firstLine = String(row.error ?? '').split(/\r?\n/u)[0];
  const match = /^(?:Error: )?CLI timeout: (.+)$/u.exec(firstLine);
  const elapsed = row.failure?.elapsed_ms;
  const timedOut = row.timed_out === true || row.failure?.timed_out === true || (match != null && Number.isFinite(elapsed)
    && Number.isFinite(timeoutMs) && timeoutMs > 0 && elapsed >= timeoutMs);
  return { metric: row.metric, classification: timedOut ? 'censored_timeout' : 'semantic_or_execution_failure',
    timeout_command: match?.[1] ?? null, timeout_ms: timedOut ? timeoutMs : null,
    recorded_elapsed_ms: elapsed ?? null, retained_unaccepted_samples: row.samples_ms?.length ?? 0,
    error: row.error ?? null, failure: row.failure ?? null };
}
for (const spec of specifications.values()) {
  const metaPath = path.join(directory, `${spec.id}.meta.json`);
  if (!fs.existsSync(metaPath)) { runs.push({ id: spec.id, status: 'pending', process_status: 'pending' }); continue; }
  const meta = read(metaPath);
  const processStatus = meta.timed_out ? 'censored' : meta.exit_code === 0 ? 'completed' : 'failed';
  const entry = { id: spec.id, status: processStatus, process_status: processStatus, exit_code: meta.exit_code,
    start: Date.parse(meta.started_at), end: Date.parse(meta.started_at) + meta.wall_seconds * 1000 };
  const issuesBeforeRun = issues.length;
  runs.push(entry);
  verify(spec.id, () => same(meta.spec, spec));
  for (const item of meta.input_hashes ?? []) {
    const planned = payloads.get(item.path.toLowerCase());
    if (planned) verify(spec.id, () => same(item.sha256.toLowerCase(), planned));
  }
  if (processStatus !== 'completed' && spec.group !== 'cli') continue;
  let rows, configuration, fixture, observedRows;
  if (spec.group === 'cli') {
    const resultsPath = path.join(directory, `${spec.id}.results.json`);
    if (!fs.existsSync(resultsPath)) {
      if (processStatus !== 'censored') issues.push({ scope: spec.id, error: 'CLI results are missing after process exit' });
      entry.status = processStatus === 'censored' ? 'censored' : 'failed';
      continue;
    }
    const report = read(resultsPath);
    observedRows = report.metrics;
    rows = observedRows.filter(row => row.correctness === 'passed');
    configuration = report.config;
    fixture = observedRows.map(row => ({ metric: row.metric, fixture_sha256: row.fixture_sha256, source_bytes: row.source_bytes }));
    const expected = expectedMetrics(spec);
    const missing = expected.filter(name => !observedRows.some(row => row.metric === name));
    const unfinished = observedRows.filter(row => row.correctness === 'pending');
    const failures = observedRows.filter(row => !['passed', 'pending'].includes(row.correctness))
      .map(row => cliFailure(row, Number(report.config.timeout_ms)));
    for (const failure of failures) {
      operationFailures.push({ run: spec.id, ...failure });
      if (failure.classification === 'semantic_or_execution_failure') issues.push({ scope: `${spec.id}/${failure.metric}`, error: failure.error ?? 'CLI operation failed without explicit timeout evidence' });
    }
    const timedOutOperations = failures.filter(failure => failure.classification === 'censored_timeout');
    const semanticFailures = failures.length - timedOutOperations.length;
    entry.report_success = report.success ?? null;
    entry.report_finalized = Boolean(report.completed_at_utc);
    entry.operation_counts = { validated: rows.length, censored: timedOutOperations.length, failed: semanticFailures,
      unfinished: unfinished.length, missing: missing.length };
    entry.validated_metrics = rows.map(row => row.metric);
    entry.unfinished_metrics = unfinished.map(row => row.metric);
    entry.missing_metrics = missing;
    const interrupted = processStatus === 'censored';
    const stoppedAfterFailure = failures.length > 0 && !Object.hasOwn(options(spec), 'continue-on-error');
    verify(spec.id, () => {
      assert.equal(report.cli_sha256.toLowerCase(), payloads.get(report.cli.toLowerCase()));
      assert.equal(report.tool_sha256.toLowerCase(), payloads.get(spec.arguments[0].toLowerCase()));
      assert.ok(observedRows.every(row => expected.includes(row.metric)), 'Unexpected CLI metric');
      assert.equal(new Set(observedRows.map(row => row.metric)).size, observedRows.length, 'Duplicate CLI metric');
      if (!interrupted) {
        assert.ok(report.completed_at_utc, 'CLI report has no completion marker after process exit');
        assert.equal(unfinished.length, 0, 'CLI report contains unfinished operations after process exit');
        if (!stoppedAfterFailure) assert.equal(missing.length, 0, 'CLI report omitted selected operations');
        assert.equal(report.success, failures.length === 0, 'CLI success marker contradicts operation results');
        if (processStatus === 'failed' && failures.length === 0) assert.fail('CLI process failed without an operation failure');
      }
    });
    // 退出成功不等于所有操作成功；超时项的任何已采样值均不进入比较。
    entry.status = semanticFailures > 0 ? 'failed'
      : interrupted || timedOutOperations.length > 0 || missing.length > 0 || unfinished.length > 0
        ? rows.length > 0 ? 'partial' : 'censored'
        : processStatus === 'completed' && report.success === true && report.completed_at_utc ? 'completed' : 'failed';
  } else {
    const entries = fs.readFileSync(path.join(directory, `${spec.id}.stdout.ndjson`), 'utf8').split(/\r?\n/u)
      .filter(line => line.trim().startsWith('{')).map(line => JSON.parse(line));
    rows = entries.filter(row => Array.isArray(row.samples_ms));
    configuration = entries.find(row => row.config)?.config;
    fixture = spec.group === 'rust' ? entries.filter(row => row.case).map(row => ({ case: row.case, source_bytes: row.source_bytes, config: row.config }))
      : entries.find(row => row.kind === 'fixture');
    if (spec.group === 'model') {
      const metadata = entries.find(row => row.mode);
      configuration = { mode: metadata.mode, samples: metadata.samples, warmups: metadata.warmups };
      fixture = entries.filter(row => row.records != null || row.parse_records != null).map(row => Object.fromEntries(
        ['metric', 'records', 'fields', 'variants', 'explicit_records_per_variant', 'parse_records', 'broken', 'bytes'].filter(key => row[key] != null).map(key => [key, row[key]])));
    }
    if (spec.group === 'mono') {
      const metadata = entries.find(row => row.kind === 'metadata');
      configuration = Object.fromEntries(['case', 'records', 'files', 'variants', 'density', 'topology', 'templates', 'operations']
        .map(key => [key, metadata[key]]));
    }
    if (['csharp', 'mono'].includes(spec.group)) verify(spec.id, () => {
      assert.ok(entries.some(row => row.kind === 'validation' && row.success === true));
      assert.ok(entries.some(row => row.kind === 'complete' && row.success === true));
      assert.ok(!entries.some(row => row.kind === 'failure'));
    });
    // 此标记对应恢复夹具后的全量重载，不能替代被测变更状态的专门回归测试。
    if (spec.group === 'rust' && spec.scenario.includes('structures')) verify(spec.id, () => {
      assert.equal(entries.filter(row => row.verification === 'fresh_reload_all_fields_and_no_diagnostics').length, rows.length);
    });
  }
  const names = rows.map(row => row.metric);
  verify(spec.id, () => {
    const identities = rows.map(row => JSON.stringify([row.metric, row.records, row.fields, row.operations_per_sample]));
    assert.equal(new Set(identities).size, identities.length, 'Duplicate metric');
    const expected = expectedMetrics(spec);
    if (expected && spec.group !== 'cli') same([...names].sort(), [...expected].sort());
    for (const row of rows) {
      assert.ok(row.samples_ms.length > 0 && row.samples_ms.every(value => Number.isFinite(value) && value >= 0));
      if (row.samples_detail) assert.equal(row.samples_detail.length, row.samples_ms.length);
      const queryMetrics = new Set(['table_lookup_batch', 'direct_property_batch', 'table_enumerate', 'dimension_cached_for_batch',
        'dimension_fallback_cached_batch', 'dimension_variants_batch', 'dimension_has_variant_batch']);
      const count = spec.group === 'rust' ? Number(spec.environment.MATRIX_SAMPLES) : spec.group === 'model' ? Number(spec.environment.ENGINE_SAMPLES)
        : row.metric === 'startup_load_text' ? 1 : Number(options(spec)[queryMetrics.has(row.metric) ? 'query-samples' : 'samples']);
      if (Number.isSafeInteger(count)) assert.equal(row.samples_ms.length, count, `Unexpected sample count: ${row.metric}`);
    }
  });
  entry.audit_issues = issues.length - issuesBeforeRun;
  if (entry.audit_issues > 0) entry.status = 'failed';
  // 已明确超时的报告可提供其他通过全部验证的指标，但不能成为完整运行对。
  if (entry.audit_issues === 0) completed.set(spec.id, { spec, configuration, fixture, rows, status: entry.status, input_hashes: meta.input_hashes });
  entry.metrics = rows.length;
}
const pairs = [];
const partialPairs = [];
for (const baseline of completed.values()) {
  if (baseline.spec.arm !== 'baseline') continue;
  const optimized = [...completed.values()].find(item => item.spec.scenario === baseline.spec.scenario && item.spec.round === baseline.spec.round && item.spec.arm === 'optimized');
  if (!optimized) continue;
  const scope = `${baseline.spec.scenario}-r${baseline.spec.round}`;
  const wholePair = baseline.status === 'completed' && optimized.status === 'completed';
  const rowIdentity = row => JSON.stringify([row.metric, row.records, row.fields, row.operations_per_sample]);
  const matching = baseline.rows.flatMap(row => {
    const other = optimized.rows.find(next => rowIdentity(next) === rowIdentity(row));
    return other ? [[row, other]] : [];
  });
  if (!matching.length) continue;
  const okay = verify(scope, () => {
    same(baseline.spec.environment, optimized.spec.environment);
    same(options(baseline.spec), options(optimized.spec));
    same(baseline.configuration, optimized.configuration);
    if (wholePair) {
      same(baseline.fixture, optimized.fixture);
      assert.equal(matching.length, baseline.rows.length);
      assert.equal(matching.length, optimized.rows.length);
    }
    for (const [before, after] of matching) {
      same(before.samples_ms.length, after.samples_ms.length);
      if (baseline.spec.group === 'cli') same(
        baseline.fixture.find(item => item.metric === before.metric), optimized.fixture.find(item => item.metric === after.metric));
    }
    const shared = new Map(baseline.input_hashes.map(item => [item.path.toLowerCase(), item.sha256.toLowerCase()]));
    for (const item of optimized.input_hashes) if (shared.has(item.path.toLowerCase())) same(shared.get(item.path.toLowerCase()), item.sha256.toLowerCase());
  });
  if (okay) (wholePair ? pairs : partialPairs).push({ scenario: baseline.spec.scenario, round: baseline.spec.round,
    metrics: matching.length, ...(wholePair ? {} : { baseline_status: baseline.status, optimized_status: optimized.status,
      validated_metrics: matching.map(([row]) => row.metric) }) });
}
const timed = runs.filter(run => run.start != null).sort((a, b) => a.start - b.start);
for (let i = 1; i < timed.length; i++) verify('serial_execution', () => assert.ok(timed[i].start >= timed[i - 1].end - 2,
  `Overlapping process intervals: ${timed[i - 1].id}, ${timed[i].id}`));
console.log(JSON.stringify({ directory, audited_at: new Date().toISOString(), audit_status: issues.length ? 'issues_found' : runs.some(run => run.status === 'pending') ? 'incomplete'
  : runs.some(run => ['censored', 'partial', 'failed'].includes(run.status)) ? 'complete_with_censored_or_failed_runs' : 'complete',
  method: 'Read-only audit of archived plans, metadata and operation results. Only fully validated operation metrics are eligible for pairs; partial runs remain separate. Does not rerun benchmarks or hash current binaries.',
  counts: Object.fromEntries(['pending', 'completed', 'partial', 'censored', 'failed'].map(status => [status, runs.filter(run => run.status === status).length])),
  process_counts: Object.fromEntries(['pending', 'completed', 'censored', 'failed'].map(status => [status, runs.filter(run => run.process_status === status).length])),
  completed_pairs: pairs, partial_pairs: partialPairs, operation_failures: operationFailures, issues, runs }, null, 2));
process.exitCode = issues.length ? 1 : 0;
