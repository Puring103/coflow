import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) options.set(process.argv[index], process.argv[index + 1]);
const output = path.resolve(options.get('--output') ?? path.join(repository, 'docs/archive/benchmarks/non-editor-2026-10-28'));
const cliExecutable = path.resolve(options.get('--cli') ?? path.join(repository, 'target/release/coflow.exe'));
const runtimeCases = 'load_checks,load_no_checks,field_lookup,record_scan,references,validate_same,validate_changed,validate_invalid,refresh_same,error_recovery,set_scalar,set_dependency,set_noop,unset_field,batch_grouped,batch_interleaved,array_element,array_replace,dict_element,dict_replace,nested_field,option_field,insert_record,delete_record,rename_record,swap_records,transfer_record,dimension_set,dimension_add,dimension_clear,dimension_lookup,external_conflict';
const plan = [];
for (const records of [1000, 10000]) {
  plan.push({ id: `engine-model-${records}`, group: 'model', executable: path.join(repository, 'target/release/examples/engine_perf.exe'), arguments: [],
    environment: { ENGINE_MODE: 'model', ENGINE_RECORDS: String(records), ENGINE_DIMENSION_RECORDS: String(records), ENGINE_SAMPLES: '3' } });
}
function runtime(id, parameters = {}) {
  const values = { RECORDS: 10000, FILES: 10, FIELDS: 20, SAMPLES: 3, WARMUPS: 1, BATCH: 1000,
    COLLECTION: 8, VARIANTS: 0, STRIDE: 100, TOPOLOGY: 'ring', TEMPLATES: 1, QUERY_BATCH: 1000,
    CASES: 'load_checks,load_no_checks,set_scalar,batch_grouped,batch_interleaved', ...parameters };
  plan.push({ id, group: 'rust', executable: path.join(repository, 'target/release/examples/runtime_matrix.exe'), arguments: [],
    environment: Object.fromEntries(Object.entries(values).map(([key, value]) => [`MATRIX_${key}`, String(value)])) });
}
function cli(id, parameters = {}) {
  const values = { records: 10000, files: 10, fields: 20, samples: 3, warmups: 1, batch: 1000,
    variants: 1, coverage: 'sparse', topology: 'sparse', case: 'all', 'timeout-ms': 300000, ...parameters };
  plan.push({ id, group: 'cli', executable: process.execPath,
    timeout_seconds: 1800,
    timeout_scope: `Whole Node CLI-matrix run including fixture/reset, warmup, timed commands and correctness queries. Each individual CLI invocation has a separate ${values['timeout-ms']}ms bound; neither timeout is a completed latency.`,
    memory_scope: 'Node CLI-matrix driver only; this memory metric excludes coflow.exe descendants and must not be reported as CLI memory. Use measure-cli-memory.mjs for direct CLI measurements.',
    arguments: [path.join(repository, 'scripts/performance/cli-matrix.mjs'), '--cli', cliExecutable,
      '--output', path.join(output, `${id}.results.json`), ...Object.entries(values).flatMap(([key, value]) => [`--${key}`, String(value)])] });
  if (values['continue-on-error']) plan.at(-1).continue_after_failure = true;
}
function csharp(id, parameters = {}) {
  const values = { case: 'scalar20', records: 10000, files: 100, samples: 5, 'query-samples': 100,
    'query-batch': 10000, variants: 1, density: 'sparse', topology: 'none', templates: false,
    operations: 'startup,load,io,reload,queries,error', ...parameters };
  plan.push({ id, group: 'csharp', executable: 'dotnet',
    arguments: [path.join(repository, 'runtimes/csharp/Coflow.Loader.Benchmarks/bin/Release/net8.0/Coflow.Loader.Benchmarks.dll'),
      ...Object.entries(values).flatMap(([key, value]) => [`--${key}`, String(value)])] });
}

// 操作矩阵覆盖常见行为；容量专项保留全部参数，避免把不同夹具相除解释为优化收益。
runtime('rust-all-1k', { RECORDS: 1000, BATCH: 100, VARIANTS: 1, CASES: runtimeCases });
runtime('rust-common-10k', { VARIANTS: 1, CASES: runtimeCases.split(',').filter(name => !['validate_invalid', 'error_recovery'].includes(name)).join(',') });
for (const records of [2000, 10000]) {
  runtime(`rust-invalid-${records}`, { RECORDS: records, VARIANTS: 1, SAMPLES: 1, WARMUPS: 0, CASES: 'validate_invalid' });
  plan.at(-1).timeout_seconds = 120;
  plan.at(-1).allow_timeout = true;
}
runtime('rust-recovery-10k', { VARIANTS: 1, SAMPLES: 1, WARMUPS: 0, CASES: 'error_recovery' });
plan.at(-1).timeout_seconds = 120;
plan.at(-1).allow_timeout = true;
runtime('rust-invalid-no-templates-10k', { VARIANTS: 1, TEMPLATES: 0, CASES: 'validate_invalid', SAMPLES: 3 });
runtime('rust-invalid-fanin-10k', { VARIANTS: 1, TOPOLOGY: 'star', CASES: 'validate_invalid', SAMPLES: 3 });
runtime('rust-main-100k', { RECORDS: 100000, FILES: 100 });
runtime('rust-structures-100k', { RECORDS: 100000, FILES: 100, CASES: 'insert_record,delete_record,rename_record,transfer_record' });
// 复杂夹具单文件十万条超过语法节点上限，健康单文件与结构拒绝分别采样。
runtime('rust-singlefile-10k', { RECORDS: 10000, FILES: 1, CASES: 'load_checks,set_scalar,batch_grouped' });
for (const records of [17000, 20000]) {
  runtime(`rust-budget-${records}`, { RECORDS: records, FILES: 1, SAMPLES: 1, WARMUPS: 0, TOPOLOGY: 'star', TEMPLATES: 0, CASES: 'load_budget_rejected' });
  plan.at(-1).timeout_seconds = 120;
  plan.at(-1).allow_timeout = true;
}
runtime('rust-width8-10k', { FIELDS: 8 });
runtime('rust-width100-10k', { FIELDS: 100 });
runtime('rust-no-templates-10k', { TEMPLATES: 0, CASES: 'load_checks,set_scalar' });
runtime('rust-fanin-10k', { TOPOLOGY: 'star', CASES: 'load_checks,set_scalar,set_dependency' });
runtime('rust-array1000-1k', { RECORDS: 1000, BATCH: 100, COLLECTION: 1000, CASES: 'load_checks,array_element,array_replace' });
runtime('rust-dim10-sparse-10k', { VARIANTS: 10, CASES: 'load_checks,dimension_set,dimension_add,dimension_clear,dimension_lookup' });
runtime('rust-dim10-dense-10k', { VARIANTS: 10, STRIDE: 1, CASES: 'load_checks,dimension_set,dimension_clear' });
runtime('rust-hot-100k', { RECORDS: 100000, FILES: 100, SAMPLES: 100, CASES: 'set_scalar,set_dependency,refresh_same' });
for (const [density, stride] of [['sparse', 100], ['dense', 1]]) {
  runtime(`rust-dim10-existing-${density}-10k`, { VARIANTS: 10, STRIDE: stride, CASES: 'dimension_existing_set,dimension_existing_clear' });
}

cli('cli-all-1k', { records: 1000, batch: 100 });
// 万条代表性查询/写入的重复样本保存在历史 cli-all-10k；慢路径采用有界探索样本。
const exploratory = { samples: 1, warmups: 0, 'timeout-ms': 120000, 'continue-on-error': true };
cli('cli-common-10k', { ...exploratory, case: 'set_preview,batch_same_file,batch_cross_file,set_where,array_insert,array_remove,array_move,dict_set,dict_remove,create_record,delete_record,rename_record,transfer_record,unset_field,dimension_set,dimension_add_variant,dimension_clear,source_write,reject_invalid_value,reject_invalid_source,repair_invalid_source,revision_conflict' });
cli('cli-main-100k', { ...exploratory, records: 100000, files: 100, case: 'data_get,check_rules,set_field,batch_same_file,batch_cross_file' });
cli('cli-cross-batch-100k', { ...exploratory, records: 100000, files: 100, case: 'batch_cross_file' });
cli('cli-singlefile-10k', { ...exploratory, files: 1, case: 'data_get,set_field,batch_same_file' });
cli('cli-width100-10k', { ...exploratory, fields: 100, case: 'data_get,set_field' });
cli('cli-fanin-10k', { ...exploratory, topology: 'high-fanin', case: 'data_refs,set_field_rules,rename_record' });
cli('cli-dim10-sparse-10k', { ...exploratory, variants: 10, case: 'dimension_query,dimension_set,dimension_clear' });
cli('cli-dim10-dense-10k', { ...exploratory, variants: 10, coverage: 'dense', case: 'dimension_query,dimension_set,dimension_clear' });

csharp('csharp-scalar20-1k', { records: 1000, files: 10 });
csharp('csharp-scalar20-10k');
csharp('csharp-scalar20-100k', { records: 100000 });
csharp('csharp-singlefile-100k', { records: 100000, files: 1, operations: 'load,io' });
csharp('csharp-width8-10k', { case: 'scalar8', operations: 'load,queries' });
csharp('csharp-width100-10k', { case: 'scalar100', operations: 'load,queries' });
csharp('csharp-tables32-10k', { case: 'tables32', operations: 'load,queries' });
csharp('csharp-complex-10k', { case: 'complex', topology: 'pairs' });
csharp('csharp-complex-100k', { case: 'complex', records: 100000, topology: 'pairs', samples: 3, operations: 'load,reload,queries' });
csharp('csharp-templates-10k', { case: 'complex', topology: 'pairs', templates: true, operations: 'load,queries' });
csharp('csharp-fanin-10k', { case: 'complex', topology: 'hub', templates: true, operations: 'load,queries' });
csharp('csharp-ring-10k', { case: 'complex', topology: 'ring', templates: true, operations: 'load,queries,error' });
for (const variants of [1, 10]) for (const density of ['sparse', 'dense']) {
  csharp(`csharp-dim${variants}-${density}-10k`, { case: 'dimensions', variants, density,
    topology: 'pairs', templates: true, samples: 3, operations: 'load,dimensions,error' });
}
csharp('csharp-dim1-no-templates-10k', { case: 'dimensions', topology: 'pairs', samples: 3, operations: 'load,dimensions' });
csharp('csharp-dim10-no-templates-10k', { case: 'dimensions', variants: 10, topology: 'pairs', samples: 3, operations: 'load' });
csharp('csharp-dim1-sparse-100k', { case: 'dimensions', records: 100000, topology: 'pairs', templates: true,
  samples: 3, operations: 'load,dimensions' });

fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n');
let selected = plan;
if (options.has('--group')) selected = selected.filter(item => item.group === options.get('--group'));
if (options.has('--only')) selected = selected.filter(item => options.get('--only').split(',').includes(item.id));
if (!selected.length) throw new Error('No matching benchmark cases');
if (options.get('--list') === 'true') { console.log(selected.map(item => item.id).join('\n')); process.exit(0); }
for (const spec of selected) {
  const metaPath = path.join(output, `${spec.id}.meta.json`);
  if (options.get('--resume') === 'true' && fs.existsSync(metaPath)) {
    const previous = JSON.parse(fs.readFileSync(metaPath, 'utf8').replace(/^\uFEFF/u, ''));
    if ((previous.exit_code === 0 || (spec.allow_timeout && previous.timed_out === true)) && JSON.stringify(previous.spec) === JSON.stringify(spec)) {
      console.log(`SKIP ${spec.id} (completed matching specification)`); continue;
    }
  }
  if (fs.existsSync(metaPath) || fs.existsSync(path.join(output, `${spec.id}.stdout.ndjson`)) || fs.existsSync(path.join(output, `${spec.id}.results.json`))) {
    throw new Error(`${spec.id}: existing unmatched, partial or failed evidence is preserved. Use a new --output directory to retry.`);
  }
  const specPath = path.join(output, `${spec.id}.spec.json`);
  fs.writeFileSync(specPath, JSON.stringify(spec, null, 2));
  console.log(`RUN ${spec.id}`);
  const result = spawnSync('pwsh', ['-NoProfile', '-File', path.join(repository, 'scripts/performance/run-case.ps1'),
    '-Spec', specPath, '-OutputDirectory', output, '-TimeoutSeconds', String(spec.timeout_seconds ?? 1800)], { cwd: repository, stdio: 'inherit', windowsHide: true });
  // 只有采样器明确记录的超时才可接受；程序自行返回 124 仍属于失败。
  const metadata = fs.existsSync(metaPath) ? JSON.parse(fs.readFileSync(metaPath, 'utf8').replace(/^\uFEFF/u, '')) : null;
  const boundedTimeout = result.status === 124 && spec.allow_timeout && metadata?.timed_out === true
    && JSON.stringify(metadata.spec) === JSON.stringify(spec);
  const retainedFailure = result.status !== 0 && spec.continue_after_failure && metadata
    && JSON.stringify(metadata.spec) === JSON.stringify(spec);
  if (result.error || (result.status !== 0 && !boundedTimeout && !retainedFailure)) throw new Error(`${spec.id}: ${result.error ?? `exit ${result.status}`}`);
  if (retainedFailure) console.log(`FAILED ${spec.id}: preserved case-level failure/timeout evidence; continuing the independent matrix. This is not a passing benchmark.`);
  if (boundedTimeout) console.log(`BOUNDED ${spec.id}: retained timeout evidence; limit=${spec.timeout_seconds ?? 1800}s for the whole run, not an operation latency`);
  fs.rmSync(specPath);
}
