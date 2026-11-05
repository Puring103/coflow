import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const output = path.resolve(process.argv[2] ?? path.join(repository, 'docs/archive/benchmarks/non-editor-2026-10-28'));
const records = Number(process.argv[3] ?? 100000);
const files = Number(process.argv[4] ?? 100);
const supportedCases = ['get', 'check', 'set', 'batch'];
const requestedCases = (process.argv[5] ?? supportedCases.join(',')).split(',').map(value => value.trim());
assert.ok(requestedCases.every(name => supportedCases.includes(name)), 'cases must be a comma-separated selection of get,check,set,batch');
assert.equal(new Set(requestedCases).size, requestedCases.length, 'cases must not contain duplicates');
// 选择参数只筛选场景，执行仍按固定顺序，避免批量写入污染随后读取的基线。
const selectedCases = supportedCases.filter(name => requestedCases.includes(name));
assert.ok(Number.isSafeInteger(records) && records > 0 && records < Number.MAX_SAFE_INTEGER - 2, 'records must be a positive safe integer');
assert.ok(Number.isSafeInteger(files) && files > 0 && files <= records && records % files === 0, 'records must be divisible by files');
const cli = path.resolve(process.argv[6] ?? path.join(repository, 'target/release/coflow.exe'));
const manifest = path.join(output, `cli-memory-${records}-fixture.json`);
fs.mkdirSync(output, { recursive: true });
// 重跑使用独立目录，保留失败或中断证据，不能静默覆盖已有采样。
for (const name of [path.basename(manifest), `cli-memory-${records}-verification.json`,
  ...selectedCases.flatMap(name => [`cli-memory-${records}-${name}.meta.json`, `cli-memory-${records}-${name}.stdout.ndjson`])]) {
  if (fs.existsSync(path.join(output, name))) throw new Error(`Existing evidence is preserved; choose a new output directory: ${name}`);
}
function execute(file, args) {
  const run = spawnSync(file, args, { cwd: repository, windowsHide: true, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  if (run.error || run.status !== 0) throw new Error(`${file}: ${run.error ?? `exit ${run.status}`}\n${run.stderr ?? ''}`);
  return run.stdout;
}
execute(process.execPath, [path.join(repository, 'scripts/performance/cli-matrix.mjs'), '--prepare-only', 'true',
  '--cli', cli, '--records', String(records), '--files', String(files), '--output', manifest]);
const fixture = JSON.parse(fs.readFileSync(manifest, 'utf8'));
const project = path.resolve(fixture.root);
const firstDataSource = fs.readFileSync(fixture.first_data_file);
assert.equal(fixture.cli_executed, false, 'fixture preparation must not execute the measured CLI');
const cliSha256 = createHash('sha256').update(fs.readFileSync(cli)).digest('hex');
const batch = Math.min(1000, records);
const perFile = records / files;
// 使用夹具取值范围之外的值，保证大于十万行时查询仍只命中本次写入。
const batchValue = records + 1;
const setValue = records + 2;
const ops = Array.from({ length: batch }, (_, index) => ({ op: 'set_field',
  record: { type: 'Row', id: `r${String((index % files) * perFile + Math.floor(index / files)).padStart(6, '0')}` },
  path: [{ kind: 'field', name: 'value' }], value: { kind: 'expression', source: String(batchValue) } }));
const request = path.join(project, 'memory-batch.json');
fs.writeFileSync(request, JSON.stringify({ protocol_version: 1, kind: 'operations', ops }));
function successful(response) {
  assert.equal(response.protocol_version, 1);
  assert.equal(response.status, 'success');
  assert.ok(!response.diagnostics?.some(diagnostic => diagnostic.severity === 'error'));
  return response.result;
}
function checkPassed(result) {
  assert.equal(result.structure, 'passed');
  assert.equal(result.rules, 'passed');
  assert.equal(result.complete, true);
}
function query(args) {
  return successful(JSON.parse(execute(cli, [...args, '--project', project, '--json'])));
}
function verifyRecord(result, value) {
  assert.equal(result.id, 'r000000');
  assert.equal(result.fields.value.kind, 'int');
  assert.equal(Number(result.fields.value.value), value);
}
// 单独运行真实 CLI 子进程，测峰值时不把 Node 驱动进程算进去；只作内存诊断，不混入正式延迟分布。
const cases = [
  ['get', ['data', 'get', '--type', 'Row', '--id', 'r000000']],
  ['check', ['check', '--rules']],
  ['set', ['data', 'set', '--type', 'Row', '--id', 'r000000', '--path', 'value', '--value-cfd', String(setValue), '--rules']],
  ['batch', ['apply', '--input', request, '--rules']],
];
for (const [name, args] of cases.filter(([name]) => selectedCases.includes(name))) {
  const id = `cli-memory-${records}-${name}`;
  const spec = { id, executable: cli, arguments: [...args, '--project', project, '--json'],
    environment: { COFLOW_LOG: '0' }, cli_sha256: cliSha256, fixture_sha256: fixture.fixture_sha256,
    memory_scope: 'Direct coflow.exe process for one command, excluding the Node and PowerShell drivers, fixture generation and subsequent verification. No warmup; filesystem caches are uncontrolled.',
    timeout_scope: 'One direct CLI command including startup, loading, operation and output; a timeout is censored command evidence, not a completed latency.' };
  const specPath = path.join(output, `${id}.spec.json`);
  fs.writeFileSync(specPath, JSON.stringify(spec));
  console.log(execute('pwsh', ['-NoProfile', '-File', path.join(repository, 'scripts/performance/run-case.ps1'), '-Spec', specPath, '-OutputDirectory', output]));
  const response = JSON.parse(fs.readFileSync(path.join(output, `${id}.stdout.ndjson`), 'utf8'));
  const result = successful(response);
  if (name === 'get') verifyRecord(result, 0);
  if (name === 'check') checkPassed(result);
  if (['set', 'batch'].includes(name)) {
    assert.equal(result.written, true, `Write not published: ${name}`);
    assert.equal(result.changed, true, `Write did not change data: ${name}`);
    checkPassed(result.validation);
  }
  // 每次写入立即复读；后续批量写入不能掩盖单条写入的错误。
  if (name === 'set') {
    verifyRecord(query(['data', 'get', '--type', 'Row', '--id', 'r000000', '--fields', 'value']), setValue);
    // 下一项测量恢复同一份输入，保证各项规格中的夹具摘要仍对应实际起始数据。
    fs.writeFileSync(fixture.first_data_file, firstDataSource);
  }
  fs.rmSync(specPath);
}
if (selectedCases.includes('batch')) {
  const verified = query(['data', 'list', '--type', 'Row', '--where', `value == ${batchValue}`, '--fields', 'value', '--limit', String(batch)]);
  assert.equal(verified.total, batch, 'Batch verification failed');
  assert.deepEqual(verified.items.map(item => item.id).sort(), ops.map(op => op.record.id).sort(), 'Batch changed unexpected records');
  assert.ok(verified.items.every(item => item.fields.value.kind === 'int' && Number(item.fields.value.value) === batchValue));
}
checkPassed(query(['check', '--rules']));
fs.writeFileSync(path.join(output, `cli-memory-${records}-verification.json`), JSON.stringify({ status: 'passed', selected_cases: selectedCases,
  batch_records: selectedCases.includes('batch') ? batch : 0,
  cli_sha256: cliSha256, fixture_sha256: fixture.fixture_sha256,
  scope: 'One diagnostic execution per operation; direct CLI OS cumulative peak sampled approximately every 100ms, with best-effort startup/exit observations. Last-interval peaks can be missed. Each write is verified outside the measured process. Not a latency distribution.' }, null, 2));
if (path.dirname(project) !== path.resolve(os.tmpdir()) || !path.basename(project).startsWith('coflow-cli-matrix-')) throw new Error('Unexpected fixture path');
fs.rmSync(project, { recursive: true, force: true });
