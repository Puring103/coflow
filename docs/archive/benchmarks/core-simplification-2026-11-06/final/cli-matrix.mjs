#!/usr/bin/env node
// 独立 CLI 进程基准：夹具、请求、恢复与结果复核均在计时区间外。
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const options = new Map();
for (let i = 2; i < process.argv.length; i++) {
  const key = process.argv[i];
  if (!key.startsWith('--')) throw new Error(`Unexpected argument: ${key}`);
  options.set(key.slice(2), process.argv[i + 1]?.startsWith('--') || i + 1 === process.argv.length ? true : process.argv[++i]);
}
const integer = (name, fallback, min = 1) => {
  const value = Number(options.get(name) ?? fallback);
  if (!Number.isSafeInteger(value) || value < min) throw new Error(`--${name} must be an integer >= ${min}`);
  return value;
};
const config = {
  records: integer('records', 1000), files: integer('files', 10), fields: integer('fields', 20, 8),
  samples: integer('samples', 3), warmups: integer('warmups', 1, 0), batch: integer('batch', 1000),
  variants: integer('variants', 1, 0), coverage: options.get('coverage') ?? 'sparse',
  topology: options.get('topology') ?? 'sparse', timeout_ms: integer('timeout-ms', 300000),
};
assert.ok(config.files <= config.records && config.records % config.files === 0, 'records must be divisible by files');
assert.ok(['sparse', 'dense'].includes(config.coverage));
assert.ok(['sparse', 'high-fanin', 'none'].includes(config.topology));
const cli = path.resolve(String(options.get('cli') ?? path.join(repository, 'target/release/coflow.exe')));
const output = path.resolve(String(options.get('output') ?? path.join(os.tmpdir(), `coflow-cli-${Date.now()}.json`)));
const id = (index) => `r${String(index).padStart(6, '0')}`;
const expression = (source) => ({ kind: 'expression', source: String(source) });
const record = (index) => ({ type: 'Row', id: typeof index === 'number' ? id(index) : index });
const field = (name) => [{ kind: 'field', name }];
const digest = (value) => createHash('sha256').update(value).digest('hex');
const caseNames = [
  'project_inspect', 'schema_inspect', 'data_get', 'data_list', 'filter_sort', 'data_search', 'data_refs',
  'check_structure', 'check_rules', 'stats_field', 'dimension_get', 'dimension_missing', 'dimension_diff', 'dimension_query',
  'set_field', 'set_field_rules', 'set_noop', 'set_preview', 'batch_same_file', 'batch_cross_file', 'set_where',
  'array_insert', 'array_remove', 'array_move', 'dict_set', 'dict_remove', 'create_record', 'delete_record',
  'rename_record', 'transfer_record', 'unset_field', 'dimension_set', 'dimension_add_variant', 'dimension_clear',
  'source_write', 'reject_invalid_value', 'reject_invalid_source', 'repair_invalid_source', 'revision_conflict',
];
if (options.has('list')) { console.log(JSON.stringify(caseNames, null, 2)); process.exit(0); }
if (options.has('help')) {
  console.log('node scripts/performance/cli-matrix.mjs --cli <exe> --case all|name,name --records 1000 --files 10 --fields 20 --samples 3 --warmups 1 --batch 1000 --variants 1 --coverage sparse|dense --topology sparse|high-fanin|none --output result.json [--keep] [--prepare-only true]');
  process.exit(0);
}
const selected = String(options.get('case') ?? 'all') === 'all' ? caseNames : String(options.get('case')).split(',');
for (const name of selected) assert.ok(caseNames.includes(name), `Unknown case ${name}`);

async function invoke(root, args, stdin = null) {
  // 计时包含进程创建、程序退出及输出收集；JSON 解析与断言在结束计时后执行。
  const started = process.hrtime.bigint();
  const child = spawn(cli, [...args, '--project', root, '--json'], {
    cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, COFLOW_LOG: '0' },
  });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; child.kill(); }, config.timeout_ms);
  const stdout = [], stderr = [];
  child.stdout.on('data', (chunk) => stdout.push(chunk));
  child.stderr.on('data', (chunk) => stderr.push(chunk));
  child.stdin.on('error', () => {});
  child.stdin.end(stdin);
  const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); }).finally(() => clearTimeout(timer));
  const elapsed_ms = Number(process.hrtime.bigint() - started) / 1e6;
  const out = Buffer.concat(stdout), err = Buffer.concat(stderr).toString('utf8');
  if (timedOut) throw Object.assign(new Error(`CLI timeout: ${args.join(' ')}`), { elapsed_ms, exit_code: code, stderr: err });
  let response;
  try { response = JSON.parse(out.toString('utf8')); }
  catch { throw new Error(`Invalid CLI JSON (${code}): ${out.toString('utf8').slice(0, 2000)}; ${err}`); }
  return { elapsed_ms, exit_code: code, response, stdout_bytes: out.length, stderr: err };
}
function successful(run) {
  assert.equal(run.exit_code, 0, JSON.stringify(run.response));
  assert.equal(run.response.status, 'success', JSON.stringify(run.response));
  assert.ok(!(run.response.diagnostics ?? []).some((diagnostic) => diagnostic.severity === 'error'), JSON.stringify(run.response));
}
function decode(value) {
  if (value == null) return value;
  switch (value.kind) {
    case 'int': return Number(value.value);
    case 'string': case 'bool': case 'float': return value.value;
    case 'none': return null;
    case 'some': return decode(value.value);
    case 'array': return value.items.map(decode);
    case 'dict': return Object.fromEntries(value.entries.map((entry) => [decode(entry.key), decode(entry.value)]));
    case 'object': return Object.fromEntries(Object.entries(value.fields).map(([key, item]) => [key, decode(item)]));
    case 'ref': return value.id;
    default: return value;
  }
}
async function get(root, index, fields, dimensions = []) {
  const run = await invoke(root, ['data', 'get', '--type', 'Row', '--id', typeof index === 'number' ? id(index) : index,
    '--fields', fields.join(','), ...dimensions.flatMap((item) => ['--dimension', item])]);
  successful(run);
  return Object.fromEntries(Object.entries(run.response.result.fields).map(([key, value]) => [key, decode(value)]));
}

async function makeFixture() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'coflow-cli-matrix-'));
  const originals = new Map();
  const perFile = config.records / config.files;
  const fileFor = (index) => `data/${String(Math.floor(index / perFile)).padStart(4, '0')}.cfd`;
  const declarations = [
    'value: int;', '@dimension("language") name: string;', 'description: string;', 'numbers: [int];',
    'metadata: {string: string};', 'parent: Option<&Row> = None;', 'stats: Stats;', '@dimension("platform") note: string = "default";',
  ];
  for (let i = 8; i < config.fields; i++) declarations.push(`f${i}: int = 0;`);
  originals.set('schema.cft', `data Stats { health: int = 100; speed: float = 1.0; }\ntable Row {\n${declarations.join('\n')}\ncheck Valid { value >= 0; name != ""; all n in numbers { n >= 0; } }\n}\ncheck Integrity { records(Row).len() > 0; }\n`);
  originals.set('coflow.yaml', 'schema: schema.cft\ndata: data/\ndimensions:\n  language:\n    dir: translations/\n  platform:\n    dir: platforms/\ncodegen:\n  - language: csharp\n    dir: generated/\n');
  const row = (index) => {
    const parent = index === 0 || config.topology === 'none' ? 'None' : `Some(&${id(config.topology === 'high-fanin' ? 0 : Math.floor((index - 1) / 2))})`;
    const extras = Array.from({ length: config.fields - 8 }, (_, i) => `f${i + 8}: ${i + index}`).join(', ');
    return `${id(index)}: Row { value: ${index}, name: "Row ${index}", description: f"Title {self.name}", numbers: [0, 1, 2], metadata: {"key": "value", "origin": "base"}, parent: ${parent}, stats: Stats { health: 100, speed: 1.5 }, note: "note"${extras ? `, ${extras}` : ''} }\n`;
  };
  for (let f = 0; f < config.files; f++) {
    const lines = [];
    for (let local = 0; local < perFile; local++) lines.push(row(f * perFile + local));
    originals.set(`data/${String(f).padStart(4, '0')}.cfd`, lines.join(''));
  }
  for (let variant = 0; variant < config.variants; variant++) {
    const name = variant === 0 ? 'ja' : `v${variant}`;
    const lines = [];
    for (let i = 0; i < config.records; i += config.coverage === 'dense' ? 1 : 10) lines.push(`${id(i)}: Row { name: "${name} Row ${i}" }`);
    originals.set(`translations/${name}.cfd`, `language[${name}] {\n${lines.join('\n')}\n}\n`);
  }
  if (config.variants > 0) originals.set('platforms/mobile.cfd', 'platform[mobile] { r000000: Row { note: "mobile" } }\n');
  const write = async (relative, text) => { const target = path.join(root, relative); await fs.mkdir(path.dirname(target), { recursive: true }); await fs.writeFile(target, text); };
  const reset = async () => {
    // 仅删除本工具创建的临时夹具子目录；不接受用户工程路径作为清理目标。
    for (const name of ['data', 'translations', 'platforms']) {
      const target = path.resolve(root, name);
      assert.equal(path.dirname(target), root);
      await fs.rm(target, { recursive: true, force: true });
    }
    for (const [name, text] of originals) await write(name, text);
  };
  await reset();
  return { root, originals, perFile, fileFor, write, reset,
    source_bytes: [...originals.values()].reduce((total, text) => total + Buffer.byteLength(text), 0),
    fixture_sha256: digest([...originals].map(([name, text]) => `${name}\0${text}`).join('\0')) };
}
async function inputFingerprint(fixture) {
  const entries = [];
  const add = async (relative) => {
    const absolute = path.join(fixture.root, relative);
    try { entries.push([relative.replaceAll('\\', '/'), digest(await fs.readFile(absolute))]); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  };
  const visit = async (relative) => {
    let children;
    try { children = await fs.readdir(path.join(fixture.root, relative), { withFileTypes: true }); }
    catch (error) { if (error.code === 'ENOENT') return; throw error; }
    for (const child of children) {
      const name = path.join(relative, child.name);
      if (child.isDirectory()) await visit(name);
      else if (child.isFile() && path.extname(child.name).toLowerCase() === '.cfd') await add(name);
    }
  };
  // 枚举完整实际输入集合；源摘要同时覆盖内容、删除与新增匹配文件。
  await add('coflow.yaml'); await add('schema.cft');
  for (const directory of ['data', 'translations', 'platforms']) await visit(directory);
  entries.sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0);
  return digest(JSON.stringify(entries));
}
if (options.has('prepare-only')) {
  // 独立进程内存诊断复用同一夹具；此入口只准备文件并保留目录，不启动CLI。
  const fixture = await makeFixture();
  const manifest = { protocol_version: 1, suite: 'cli_matrix_fixture', prepared_at_utc: new Date().toISOString(),
    root: fixture.root, config, source_bytes: fixture.source_bytes, fixture_sha256: fixture.fixture_sha256,
    file_for_zero: fixture.fileFor(0), first_data_file: path.join(fixture.root, fixture.fileFor(0)),
    retained: true, cli_executed: false };
  await fs.mkdir(path.dirname(output), { recursive: true });
  await fs.writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(JSON.stringify(manifest));
  process.exit(0);
}
await fs.access(cli);
const apply = (ops) => ({ args: ['apply', '--input', '-', '--rules'], stdin: JSON.stringify({ protocol_version: 1, kind: 'operations', ops }) });
const set = (index, name, value) => ({ op: 'set_field', record: record(index), path: field(name), value: expression(value) });
const readOnly = new Set(caseNames.slice(0, caseNames.indexOf('set_field')));

async function prepare(name, fixture, iteration) {
  const { root, fileFor, perFile, originals } = fixture;
  const next = 1000000 + iteration;
  const result = { args: [], stdin: null, expected_exit: [0], expected_changed: null, verify: async () => {}, affected: 0 };
  const setValue = (target = 0, key = 'value', value = next, source = String(value)) => {
    result.args = ['data', 'set', '--type', 'Row', '--id', id(target), '--path', key, '--value-cfd', source];
    result.verify = async () => assert.deepEqual((await get(root, target, [key]))[key], value);
    result.expected_changed = true; result.affected = 1;
  };
  const operation = (op, verify) => { Object.assign(result, apply([op])); result.verify = verify; result.expected_changed = true; result.affected = 1; };
  if (name === 'project_inspect') result.args = ['project', 'inspect'];
  if (name === 'schema_inspect') result.args = ['schema', 'inspect', '--type', 'Row'];
  if (name === 'data_get') result.args = ['data', 'get', '--type', 'Row', '--id', id(0), '--fields', 'value,name,description'];
  if (name === 'data_list') result.args = ['data', 'list', '--type', 'Row', '--fields', 'value,name', '--limit', '50'];
  if (name === 'filter_sort') result.args = ['data', 'list', '--type', 'Row', '--where', `value >= ${Math.floor(config.records / 2)}`, '--sort', '-value', '--fields', 'value,name', '--limit', '50'];
  if (name === 'data_search') result.args = ['data', 'search', '--type', 'Row', '--text', `Row ${config.records - 1}`, '--fields', 'name', '--limit', '50'];
  if (name === 'data_refs') result.args = ['data', 'refs', '--type', 'Row', '--id', id(0), '--direction', 'incoming', '--limit', '50'];
  if (name === 'check_structure') result.args = ['check'];
  if (name === 'check_rules') result.args = ['check', '--rules'];
  if (name === 'stats_field') result.args = ['stats', 'field', '--type', 'Row', '--field', 'value', '--metrics', 'count,min,max,mean'];
  if (name === 'dimension_get') result.args = ['dimension', 'get', '--type', 'Row', '--id', id(0), '--field', 'name', '--dimension', 'language', '--variant', 'ja'];
  if (name === 'dimension_missing') result.args = ['dimension', 'missing', '--type', 'Row', '--dimension', 'language', '--variant', 'ja', '--limit', '50'];
  if (name === 'dimension_diff') result.args = ['dimension', 'diff', '--type', 'Row', '--dimension', 'language', '--from-base', '--to', 'ja', '--limit', '50'];
  if (name === 'dimension_query') result.args = ['data', 'get', '--type', 'Row', '--id', id(0), '--dimension', 'language=ja', '--dimension', 'platform=mobile', '--fields', 'name,description,note'];
  if (['set_field', 'set_field_rules', 'set_preview', 'set_noop', 'reject_invalid_value', 'revision_conflict'].includes(name)) {
    setValue();
    if (name === 'set_field_rules') result.args.push('--rules');
    if (name === 'set_preview') { result.args.push('--dry-run'); result.verify = async () => assert.equal((await get(root, 0, ['value'])).value, 0); }
    if (name === 'set_noop') { setValue(0, 'value', 0); result.expected_changed = false; }
    if (name === 'reject_invalid_value') { result.args[result.args.length - 1] = '"invalid"'; result.expected_exit = [1, 2]; result.expected_changed = null; }
    if (name === 'revision_conflict') { result.args.push('--if-revision', 'stale-benchmark-revision'); result.expected_exit = [3]; result.expected_changed = null; }
  }
  if (['batch_same_file', 'batch_cross_file', 'set_where'].includes(name)) {
    const count = Math.min(config.batch, name === 'batch_same_file' ? perFile : config.records);
    const ids = Array.from({ length: count }, (_, i) => name === 'batch_cross_file' ? (i % config.files) * perFile + Math.floor(i / config.files) : i);
    Object.assign(result, apply(name === 'set_where' ? [{ op: 'set_where', selector: { type: 'Row', filter: `value < ${count}` }, path: field('value'), value: expression(next), expected_count: count }] : ids.map((index) => set(index, 'value', next))));
    result.affected = count; result.expected_changed = true;
    result.verify = async () => {
      const actual = await invoke(root, ['data', 'list', '--type', 'Row', '--where', `value == ${next}`, '--fields', 'value', '--limit', String(count)]);
      successful(actual); assert.equal(actual.response.result.total, count);
      assert.deepEqual(actual.response.result.items.map((item) => item.id).sort(), ids.map(id).sort());
    };
  }
  if (name === 'array_insert') operation({ op: name, record: record(0), path: field('numbers'), index: 1, value: expression(next) }, async () => assert.deepEqual((await get(root, 0, ['numbers'])).numbers, [0, next, 1, 2]));
  if (name === 'array_remove') operation({ op: name, record: record(0), path: field('numbers'), index: 1 }, async () => assert.deepEqual((await get(root, 0, ['numbers'])).numbers, [0, 2]));
  if (name === 'array_move') operation({ op: name, record: record(0), path: field('numbers'), from: 0, to: 2 }, async () => assert.deepEqual((await get(root, 0, ['numbers'])).numbers, [1, 2, 0]));
  if (name === 'dict_set') operation({ op: name, record: record(0), path: field('metadata'), key: expression('"key"'), value: expression(`"edited${iteration}"`) }, async () => assert.equal((await get(root, 0, ['metadata'])).metadata.key, `edited${iteration}`));
  if (name === 'dict_remove') operation({ op: name, record: record(0), path: field('metadata'), key: expression('"key"') }, async () => assert.deepEqual((await get(root, 0, ['metadata'])).metadata, { origin: 'base' }));
  if (name === 'create_record') operation({ op: name, record: record('added'), file: fileFor(0), fields: Object.fromEntries(Object.entries({ value: String(next), name: '"Added"', description: 'f"Title {self.name}"', numbers: '[]', metadata: '{}', stats: 'Stats {}' }).map(([key, value]) => [key, expression(value)])) }, async () => assert.deepEqual(await get(root, 'added', ['value', 'name']), { value: next, name: 'Added' }));
  if (name === 'delete_record') operation({ op: name, record: record(config.records - 1) }, async () => { const run = await invoke(root, ['data', 'list', '--type', 'Row', '--id', id(config.records - 1)]); successful(run); assert.equal(run.response.result.total, 0); });
  if (name === 'rename_record') operation({ op: name, record: record(0), new_id: 'renamed' }, async () => {
    assert.equal((await get(root, 'renamed', ['value'])).value, 0);
    if (config.records > 1 && config.topology !== 'none') assert.equal((await get(root, 1, ['parent'])).parent, 'renamed');
  });
  if (name === 'transfer_record') operation({ op: name, record: record(0), to_file: 'data/transferred.cfd' }, async () => {
    const run = await invoke(root, ['data', 'get', '--type', 'Row', '--id', id(0), '--fields', 'value']); successful(run);
    assert.equal(run.response.result.file, 'data/transferred.cfd'); assert.equal(decode(run.response.result.fields.value), 0);
  });
  if (name === 'unset_field') operation({ op: name, record: record(0), path: field('note') }, async () => assert.equal((await get(root, 0, ['note'])).note, 'default'));
  if (['dimension_set', 'dimension_add_variant', 'dimension_clear'].includes(name)) {
    const variant = name === 'dimension_add_variant' ? 'added' : 'ja';
    const clear = name === 'dimension_clear';
    if (clear && config.variants === 0) await fixture.write('translations/ja.cfd', 'language[ja] { r000000: Row { name: "ja Row 0" } }\n');
    operation({ op: clear ? 'clear_dimension_value' : 'set_dimension_value', record: record(0), dimension: 'language', variant, field: 'name', ...(clear ? {} : { value: expression(`"Localized ${iteration}"`) }) }, async () => {
      const run = await invoke(root, ['dimension', 'get', '--type', 'Row', '--id', id(0), '--field', 'name', '--dimension', 'language', '--variant', variant]); successful(run);
      assert.equal(run.response.result.explicit, !clear);
      assert.equal(decode(run.response.result.effective), clear ? 'Row 0' : `Localized ${iteration}`);
    });
  }
  if (['source_write', 'reject_invalid_source', 'repair_invalid_source'].includes(name)) {
    const before = originals.get(fileFor(0));
    const changed = before.replace('value: 0,', `value: ${next},`);
    const text = name === 'reject_invalid_source' ? before.replace('value: 0,', 'value: "bad",') : changed;
    if (name === 'repair_invalid_source') await fixture.write(fileFor(0), before.replace('value: 0,', 'value: "bad",'));
    await fs.writeFile(path.join(root, 'request.cfd'), text);
    result.args = ['source', 'write', '--file', fileFor(0), '--input', path.join(root, 'request.cfd'), '--rules'];
    result.expected_changed = true; result.affected = name === 'reject_invalid_source' ? 0 : 1;
    result.submitted_file_records = perFile;
    result.verify = async () => assert.equal((await get(root, 0, ['value'])).value, next);
    if (name === 'reject_invalid_source') { result.expected_exit = [1]; result.expected_changed = null; }
  }
  if (result.expected_exit[0] !== 0) result.verify = async () => assert.equal((await get(root, 0, ['value'])).value, 0);
  assert.ok(result.args.length, name);
  return result;
}

function validateQuery(name, response) {
  const result = response.result;
  if (name === 'project_inspect') assert.ok(result.config);
  if (name === 'schema_inspect') assert.ok(result.types.some((type) => type.name === 'Row'));
  if (name === 'data_get') { assert.equal(result.id, id(0)); assert.equal(decode(result.fields.value), 0); assert.equal(result.rendered.description, 'Title Row 0'); }
  if (name === 'data_list') { assert.equal(result.total, config.records); assert.equal(result.items.length, Math.min(50, config.records)); }
  if (name === 'filter_sort') { assert.equal(result.total, config.records - Math.floor(config.records / 2)); assert.equal(decode(result.items[0].fields.value), config.records - 1); }
  if (name === 'data_search') assert.ok(result.items.some((item) => item.id === id(config.records - 1)));
  if (name === 'data_refs') {
    const expected = config.topology === 'none' ? 0 : config.topology === 'high-fanin' ? config.records - 1 : Math.min(2, config.records - 1);
    assert.equal(result.total, expected);
    assert.ok(result.items.every((item) => item.target.id === id(0)));
  }
  if (name === 'check_structure' || name === 'check_rules') {
    assert.equal(result.structure, 'passed'); assert.equal(result.complete, true);
    assert.equal(result.rules, name === 'check_rules' ? 'passed' : 'not_executed');
  }
  if (name === 'stats_field') {
    assert.equal(result.count, config.records); assert.equal(Number(result.min), 0); assert.equal(Number(result.max), config.records - 1);
    assert.equal(Number(result.mean.numerator) / result.mean.denominator, (config.records - 1) / 2);
  }
  if (name === 'dimension_get') { assert.equal(result.explicit, config.variants > 0); assert.equal(decode(result.effective), config.variants > 0 ? 'ja Row 0' : 'Row 0'); }
  if (name === 'dimension_missing' || name === 'dimension_diff') {
    const covered = config.variants === 0 ? 0 : Math.ceil(config.records / (config.coverage === 'dense' ? 1 : 10));
    assert.equal(result.total, name === 'dimension_missing' ? config.records - covered : covered);
  }
  if (name === 'dimension_query') {
    assert.equal(decode(result.fields.name), config.variants > 0 ? 'ja Row 0' : 'Row 0');
    assert.equal(result.rendered.description, config.variants > 0 ? 'Title ja Row 0' : 'Title Row 0');
    assert.equal(decode(result.fields.note), config.variants > 0 ? 'mobile' : 'note');
  }
}

const report = {
  protocol_version: 1, suite: 'cli_matrix', started_at_utc: new Date().toISOString(), config,
  cli, cli_sha256: digest(await fs.readFile(cli)), tool_sha256: digest(await fs.readFile(fileURLToPath(import.meta.url))),
  environment: { platform: process.platform, architecture: process.arch, node: process.version, cpu: os.cpus()[0]?.model, logical_cpus: os.cpus().length, total_memory_bytes: os.totalmem() },
  timing: 'Independent CLI process spawn through close and stdout collection; fixture generation/reset, input construction, JSON parsing and correctness checks excluded. Filesystem cache is warm; no OS cache eviction.',
  validation: 'Every sample validates exit/status, operation results and fresh CLI semantic queries. Nonpublishing cases compare the entire selected input path/content fingerprint, including added/deleted matching files. Each case ends with full check --rules. Original fixture is restored before each mutating sample.',
  memory: { collected: false, reason: 'Node parent memory does not represent the CLI child. No child working-set sampler is included.' },
  metrics: [],
};
await fs.mkdir(path.dirname(output), { recursive: true });
const persist = async () => fs.writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
let failed = false;
for (const name of selected) {
  const fixture = await makeFixture();
  const metric = { metric: name, records: config.records, files: config.files, fields: config.fields,
    fixture_sha256: fixture.fixture_sha256, source_bytes: fixture.source_bytes,
    samples_ms: [], samples_detail: [], warmups_ms: [], correctness: 'pending' };
  report.metrics.push(metric);
  console.error(`RUN ${name} records=${config.records} files=${config.files}`);
  try {
    const initial = await invoke(fixture.root, ['check', '--rules']); successful(initial);
    for (let iteration = -config.warmups; iteration < config.samples; iteration++) {
      if (!readOnly.has(name)) await fixture.reset();
      const request = await prepare(name, fixture, iteration + config.warmups);
      const before = ['set_noop', 'set_preview', 'reject_invalid_value', 'reject_invalid_source', 'revision_conflict'].includes(name)
        ? await inputFingerprint(fixture) : null;
      const run = await invoke(fixture.root, request.args, request.stdin);
      assert.ok(request.expected_exit.includes(run.exit_code), JSON.stringify(run.response));
      if (run.exit_code === 0) successful(run);
      else assert.notEqual(run.response.status, 'success');
      if (request.expected_changed !== null) assert.equal(run.response.result.changed, request.expected_changed, JSON.stringify(run.response));
      if (readOnly.has(name)) validateQuery(name, run.response);
      if (before !== null) assert.equal(await inputFingerprint(fixture), before, 'nonpublishing operation changed selected input contents or file set');
      await request.verify();
      if (iteration < 0) metric.warmups_ms.push(run.elapsed_ms);
      else {
        metric.samples_ms.push(run.elapsed_ms);
        metric.samples_detail.push({ elapsed_ms: run.elapsed_ms, exit_code: run.exit_code, status: run.response.status,
          stdout_bytes: run.stdout_bytes, changed: run.response.result.changed ?? null, written: run.response.result.written ?? null,
          affected_records: request.affected, submitted_file_records: request.submitted_file_records ?? null,
          revision: run.response.revision ?? null, diagnostic_count: run.response.diagnostics?.length ?? 0,
          args: request.args, stdin_sha256: request.stdin === null ? null : digest(request.stdin), stderr: run.stderr });
      }
      await persist();
    }
    const finalCheck = await invoke(fixture.root, ['check', '--rules']); successful(finalCheck);
    const times = [...metric.samples_ms].sort((a, b) => a - b);
    const middle = Math.floor(times.length / 2);
    metric.p50_ms = times.length % 2 ? times[middle] : (times[middle - 1] + times[middle]) / 2;
    metric.p95_ms = times.length >= 20 ? times[Math.ceil(times.length * .95) - 1] : null;
    metric.min_ms = times[0]; metric.max_ms = times.at(-1); metric.correctness = 'passed';
    console.error(`PASS ${name} p50=${metric.p50_ms.toFixed(3)}ms`);
  } catch (error) {
    failed = true; metric.correctness = 'failed'; metric.error = String(error.stack ?? error); metric.fixture_path = fixture.root;
    metric.failure = { elapsed_ms: error.elapsed_ms ?? null, exit_code: error.exit_code ?? null, stderr: error.stderr ?? null };
    console.error(`FAIL ${name}: ${error.message}`);
  } finally {
    if (options.has('keep')) metric.fixture_path = fixture.root;
    else if (metric.correctness === 'passed') {
      assert.ok(path.basename(fixture.root).startsWith('coflow-cli-matrix-') && path.dirname(fixture.root) === path.resolve(os.tmpdir()));
      await fs.rm(fixture.root, { recursive: true, force: true });
    }
    await persist();
  }
  if (failed && !options.has('continue-on-error')) break;
}
report.completed_at_utc = new Date().toISOString(); report.success = !failed;
await persist();
console.log(JSON.stringify({ output, success: report.success, metrics: report.metrics.length }));
process.exitCode = failed ? 1 : 0;
