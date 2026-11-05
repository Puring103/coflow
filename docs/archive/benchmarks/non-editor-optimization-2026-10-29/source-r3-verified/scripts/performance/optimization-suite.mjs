// 固定旧/新产物交替采样；每轮独立进程，失败与超时原样保留。
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) options.set(process.argv[index], process.argv[index + 1]);
const output = path.resolve(options.get('--output') ?? path.join(repo, 'docs/archive/benchmarks/non-editor-optimization-2026-10-29'));
const roots = {
  baseline: path.resolve(options.get('--baseline') ?? path.join(repo, 'target/performance-baseline-20261029')),
  optimized: path.resolve(options.get('--optimized') ?? path.join(repo, 'target/performance-optimized-20261029')),
};
const players = {
  baseline: path.join(repo, 'target/benchmarks/unity-control-20261029-mono/CoflowBenchmark.exe'),
  optimized: path.resolve(options.get('--mono') ?? path.join(repo, 'target/benchmarks/unity-optimized-20261029-mono/CoflowBenchmark.exe')),
};
const sourcePlan = JSON.parse(fs.readFileSync(path.join(repo, 'docs/archive/benchmarks/non-editor-2026-10-28/plan.json'), 'utf8').replace(/^\uFEFF/u, ''));
const scenarios = [];
function original(id) {
  const item = sourcePlan.find(item => item.id === id);
  if (!item) throw new Error(`Missing original specification: ${id}`);
  return structuredClone(item);
}
function rust(id, source, parameters = {}) {
  const item = original(source);
  scenarios.push({ id, group: item.group, environment: { ...item.environment, MATRIX_SAMPLES: '1', MATRIX_WARMUPS: '0', ...parameters } });
}
rust('rust-all-1k', 'rust-all-1k');
rust('rust-common-10k', 'rust-common-10k', { MATRIX_CASES: 'load_checks,load_no_checks,set_scalar,set_noop,batch_grouped,batch_interleaved,insert_record,delete_record,rename_record,transfer_record,refresh_same' });
rust('rust-main-100k', 'rust-main-100k');
rust('rust-structures-100k', 'rust-structures-100k');
rust('rust-structures-literal-100k', 'rust-structures-100k', { MATRIX_TEMPLATES: '0' });
rust('rust-hot-100k', 'rust-hot-100k', { MATRIX_SAMPLES: '100', MATRIX_WARMUPS: '1' });
rust('rust-budget-20000', 'rust-budget-20000');
rust('rust-invalid-2000', 'rust-invalid-2000');
rust('rust-invalid-10000', 'rust-invalid-10000');
rust('rust-invalid-100000', 'rust-invalid-10000', { MATRIX_RECORDS: '100000', MATRIX_FILES: '100' });
scenarios.at(-1).optimizedOnly = true;
rust('rust-invalid-no-templates-10k', 'rust-invalid-no-templates-10k');
rust('rust-invalid-fanin-10k', 'rust-invalid-fanin-10k');
rust('rust-recovery-10k', 'rust-recovery-10k');
rust('rust-width100-10k', 'rust-width100-10k');
rust('rust-singlefile-10k', 'rust-singlefile-10k');
rust('rust-dim10-existing-sparse-10k', 'rust-dim10-existing-sparse-10k');
rust('rust-dim10-existing-dense-10k', 'rust-dim10-existing-dense-10k');
scenarios.push({ id: 'engine-model-10000', group: 'model', environment: { ENGINE_MODE: 'model', ENGINE_RECORDS: '10000', ENGINE_DIMENSION_RECORDS: '10000', ENGINE_SAMPLES: '3' } });

const cliDefaults = { records: 10000, files: 10, fields: 20, samples: 1, warmups: 0, batch: 1000, variants: 1, coverage: 'sparse', topology: 'sparse', 'timeout-ms': 120000, 'continue-on-error': true };
function cli(id, parameters) { scenarios.push({ id, group: 'cli', parameters: { ...cliDefaults, ...parameters } }); }
cli('cli-all-1k', { records: 1000, batch: 100, case: 'all' });
cli('cli-common-10k', { case: 'data_get,check_rules,set_field,set_field_rules,set_noop,set_preview,batch_same_file,batch_cross_file' });
cli('cli-main-100k', { records: 100000, files: 100, case: 'data_get,check_rules,set_field,batch_same_file,batch_cross_file' });
cli('cli-width100-10k', { fields: 100, case: 'data_get,set_field' });
cli('cli-dim10-sparse-10k', { variants: 10, case: 'dimension_query,dimension_set,dimension_clear' });
cli('cli-dim10-dense-10k', { variants: 10, coverage: 'dense', case: 'dimension_query,dimension_set,dimension_clear' });

const managedDefaults = { case: 'scalar20', records: 10000, files: 100, samples: 1, 'query-samples': 100, 'query-batch': 10000, variants: 1, density: 'sparse', topology: 'none', templates: false, operations: 'load,queries,error' };
function managed(id, parameters) {
  for (const group of ['csharp', 'mono']) scenarios.push({ id: `${group}-${id}`, group, parameters: { ...managedDefaults, ...parameters } });
}
managed('scalar20-1k', { records: 1000, files: 10, operations: 'startup,load,io,reload,queries,error' });
managed('scalar20-100k', { records: 100000, operations: 'load,io,reload,queries,error' });
managed('width8-10k', { case: 'scalar8', operations: 'load,queries' });
managed('width100-10k', { case: 'scalar100', operations: 'load,queries' });
managed('tables32-10k', { case: 'tables32', operations: 'load,queries' });
managed('complex-100k', { case: 'complex', records: 100000, topology: 'pairs', operations: 'load,reload,queries' });
managed('templates-10k', { case: 'complex', topology: 'pairs', templates: true });
managed('ring-10k', { case: 'complex', topology: 'ring', templates: true });
managed('dim1-sparse-10k', { case: 'dimensions', topology: 'pairs', templates: true, operations: 'load,dimensions,error' });
managed('dim10-sparse-10k', { case: 'dimensions', variants: 10, topology: 'pairs', templates: true, operations: 'load' });
managed('dim10-dense-10k', { case: 'dimensions', variants: 10, density: 'dense', topology: 'pairs', templates: true, operations: 'load' });
managed('dim10-no-templates-10k', { case: 'dimensions', variants: 10, topology: 'pairs', operations: 'load' });

let chosen = scenarios;
if (options.has('--group')) chosen = chosen.filter(item => options.get('--group').split(',').includes(item.group));
if (options.has('--only')) chosen = chosen.filter(item => options.get('--only').split(',').includes(item.id));
if (!chosen.length) throw new Error('No matching scenarios');
if (options.get('--list') === 'true') { console.log(chosen.map(item => item.id).join('\n')); process.exit(0); }
const rounds = Number(options.get('--rounds') ?? 3);
if (!Number.isSafeInteger(rounds) || rounds < 1) throw new Error('Invalid rounds');
const args = parameters => Object.entries(parameters).flatMap(([key, value]) => [`--${key}`, String(value)]);
const plans = [];
const selectedArms = (options.get('--arms') ?? 'baseline,optimized').split(',');
if (!selectedArms.length || selectedArms.some(arm => !['baseline', 'optimized'].includes(arm))) throw new Error('Invalid arms');
for (let round = 1; round <= rounds; round++) for (const scenario of chosen) {
  // 轮次交替前后顺序，避免只把机器随时间变热的一侧称为优化收益。
  for (const arm of (round % 2 ? ['baseline', 'optimized'] : ['optimized', 'baseline']).filter(arm => selectedArms.includes(arm) && (!scenario.optimizedOnly || arm === 'optimized'))) {
    const id = `${scenario.id}-r${round}-${arm}`;
    const spec = { id, scenario: scenario.id, round, arm, group: scenario.group, arguments: [], timeout_seconds: 1800 };
    if (['rust', 'model'].includes(scenario.group)) {
      spec.executable = path.join(roots[arm], scenario.group === 'rust' ? 'runtime_matrix.exe' : 'engine_perf.exe');
      spec.environment = scenario.environment;
      if (scenario.id.includes('invalid') || scenario.id.includes('recovery')) spec.timeout_seconds = 120;
    } else if (scenario.group === 'cli') {
      spec.executable = process.execPath;
      spec.arguments = [path.join(repo, 'scripts/performance/cli-matrix.mjs'), '--cli', path.join(roots[arm], 'coflow.exe'), '--output', path.join(output, `${id}.results.json`), ...args(scenario.parameters)];
      spec.memory_scope = 'Node driver only; excluded from CLI peak comparison. Direct CLI memory uses measure-cli-memory.mjs.';
    } else if (scenario.group === 'csharp') {
      spec.executable = 'dotnet';
      spec.arguments = [path.join(roots[arm], 'dotnet/Coflow.Loader.Benchmarks.dll'), ...args(scenario.parameters)];
    } else {
      spec.executable = players[arm];
      spec.arguments = ['-batchmode', '-nographics', '-logFile', path.join(output, `${id}.player.log`), '--output', path.join(output, `${id}.measurements.ndjson`), ...args(scenario.parameters)];
      spec.memory_scope = 'Actual Unity Mono player, including engine, fixtures, preflight, warmup and validation; not model retained heap.';
    }
    plans.push(spec);
  }
}
fs.mkdirSync(output, { recursive: true });
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const uniquePayloads = new Set();
for (const spec of plans) {
  if (fs.existsSync(spec.executable)) uniquePayloads.add(spec.executable);
  for (const argument of spec.arguments) if (/\.(dll|mjs|exe)$/u.test(argument) && fs.existsSync(argument)) uniquePayloads.add(argument);
  if (spec.group === 'csharp') uniquePayloads.add(path.join(path.dirname(spec.arguments[0]), 'Coflow.Loader.dll'));
  if (spec.group === 'mono') {
    uniquePayloads.add(path.join(path.dirname(spec.executable), 'CoflowBenchmark_Data/Managed/Coflow.Loader.dll'));
    uniquePayloads.add(path.join(path.dirname(spec.executable), 'CoflowBenchmark_Data/Managed/Assembly-CSharp.dll'));
  }
}
const manifest = { created_at: new Date().toISOString(), protocol: 'Serial paired independent processes; A/B then B/A by round. Full raw samples retained. OS caches uncontrolled. Builds/tests excluded.', plans, payloads: [...uniquePayloads].sort().map(file => ({ path: file, sha256: hash(file) })) };
const selectionKey = options.has('--only') ? createHash('sha256').update(options.get('--only')).digest('hex').slice(0, 12) : 'all';
const manifestPath = path.join(output, `plan-${options.get('--group') ?? 'all'}-${selectionKey}-${selectedArms.join('-')}.json`);
// 每次计划快照保留各自规格，续跑逐项核对实际执行参数和载荷摘要。
if (!fs.existsSync(manifestPath)) fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
else {
  const previous = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  if (JSON.stringify(previous.plans) !== JSON.stringify(plans) || JSON.stringify(previous.payloads) !== JSON.stringify(manifest.payloads)) throw new Error('Plan or artifacts changed; use a new output directory');
}
let failures = 0;
for (const spec of plans) {
  const meta = path.join(output, `${spec.id}.meta.json`);
  if (fs.existsSync(meta) && options.get('--resume') === 'true') {
    const old = JSON.parse(fs.readFileSync(meta, 'utf8').replace(/^\uFEFF/u, ''));
    if (JSON.stringify(old.spec) !== JSON.stringify(spec)) throw new Error(`Existing specification differs: ${spec.id}`);
    console.log(`RETAIN ${spec.id}: exit=${old.exit_code}, timeout=${old.timed_out}`);
    if (old.exit_code !== 0 || old.timed_out) failures++;
    continue;
  }
  if (fs.existsSync(meta) || fs.existsSync(path.join(output, `${spec.id}.stdout.ndjson`))) throw new Error(`Existing evidence: ${spec.id}`);
  const specPath = path.join(output, `${spec.id}.spec.json`);
  fs.writeFileSync(specPath, JSON.stringify(spec, null, 2));
  fs.writeFileSync(path.join(output, 'progress.json'), JSON.stringify({ active: spec.id, started_at: new Date().toISOString(), completed: plans.indexOf(spec), total: plans.length }));
  console.log(`RUN ${spec.id}`);
  const run = spawnSync('pwsh', ['-NoProfile', '-File', path.join(repo, 'scripts/performance/run-case.ps1'), '-Spec', specPath, '-OutputDirectory', output, '-TimeoutSeconds', String(spec.timeout_seconds)], { cwd: repo, windowsHide: true, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  const result = fs.existsSync(meta) ? JSON.parse(fs.readFileSync(meta, 'utf8').replace(/^\uFEFF/u, '')) : null;
  if (run.error || !result) throw new Error(`${spec.id}: ${run.error ?? run.stderr}`);
  // Unity 自带输出文件；合入固定后缀仅供共用汇总器读取，不改其任何测量值。
  if (spec.group === 'mono') {
    const measured = path.join(output, `${spec.id}.measurements.ndjson`);
    if (fs.existsSync(measured)) fs.appendFileSync(path.join(output, `${spec.id}.stdout.ndjson`), fs.readFileSync(measured));
  }
  console.log(`DONE ${spec.id}: exit=${result.exit_code}, timeout=${result.timed_out}, wall=${result.wall_seconds.toFixed(2)}s`);
  if (result.exit_code !== 0 || result.timed_out) failures++;
}
fs.writeFileSync(path.join(output, 'progress.json'), JSON.stringify({ active: null, completed: plans.length, total: plans.length, failed_or_censored: failures, finished_at: new Date().toISOString() }));
console.log(JSON.stringify({ completed: plans.length, failed_or_censored: failures }));
