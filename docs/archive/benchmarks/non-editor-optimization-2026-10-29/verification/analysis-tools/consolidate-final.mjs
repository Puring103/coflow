import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

// 默认从仓库工作目录解析；显式参数可在任意目录指向完整归档。
assert.ok(process.argv.length <= 3, 'Usage: node consolidate-final.mjs [archive-root]');
const archive = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29');
const output = path.join(archive, 'final-results');
const sources = [
  { directory: path.join(archive, 'final-r4'), groups: ['rust', 'model', 'cli'] },
  { directory: path.join(archive, 'managed-r6'), groups: ['csharp', 'mono'] },
  { directory: path.join(archive, 'cli-expanded-r4'), groups: ['cli'] },
];
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const digest = data => createHash('sha256').update(data).digest('hex');
assert.ok(!fs.existsSync(output), 'Do not overwrite previous final evidence');
const selected = new Map();
const specifications = new Map();
// 多个计划可引用同一进程；仅完全相同的规格和同一来源文件允许去重。
function select(source, name) {
  const previous = selected.get(name);
  if (previous) assert.equal(previous.source, source, `Conflicting source for destination: ${name}`);
  else selected.set(name, { source, name });
}
for (const source of sources) {
  const names = fs.readdirSync(source.directory);
  const plans = names.filter(name => /^plan-.*\.json$/u.test(name)).map(name => ({ name, manifest: read(path.join(source.directory, name)) }))
    .filter(plan => plan.manifest.plans.some(spec => source.groups.includes(spec.group)));
  assert.ok(plans.length, `Missing plans in ${source.directory}`);
  for (const plan of plans) {
    assert.ok(plan.manifest.plans.every(spec => source.groups.includes(spec.group)), 'Mixed-group manifest requires explicit handling');
    select(path.join(source.directory, plan.name), plan.name);
    for (const spec of plan.manifest.plans) {
      if (specifications.has(spec.id)) assert.deepEqual(specifications.get(spec.id), spec, `Conflicting specification: ${spec.id}`);
      else specifications.set(spec.id, spec);
      assert.ok(fs.existsSync(path.join(source.directory, `${spec.id}.meta.json`)), `Unfinished process: ${spec.id}`);
      for (const name of names.filter(name => name.startsWith(`${spec.id}.`) && !name.endsWith('.paused.json'))) {
        select(path.join(source.directory, name), name);
      }
    }
  }
}
fs.mkdirSync(output);
const copied = [...selected.values()].map(item => {
  const bytes = fs.readFileSync(item.source);
  fs.writeFileSync(path.join(output, item.name), bytes, { flag: 'wx' });
  assert.equal(digest(fs.readFileSync(path.join(output, item.name))), digest(bytes));
  return { file: item.name, source: path.relative(archive, item.source).replaceAll('\\', '/'), sha256: digest(bytes), bytes: bytes.length };
});
fs.writeFileSync(path.join(output, 'evidence-provenance.json'), JSON.stringify({
  created_at: new Date().toISOString(),
  method: 'Byte-identical copies of completed Rust/model/CLI R4 evidence, the extended R4 CLI coverage, and newly measured managed R6 evidence. Original specs, output paths, timestamps, failures and hashes remain unchanged. This directory is an analysis view and must not be passed to the benchmark resume command.',
  sources: sources.map(source => ({ directory: path.relative(archive, source.directory).replaceAll('\\', '/'), groups: source.groups })),
  files: copied,
}, null, 2) + '\n');
console.log(JSON.stringify({ output, files: copied.length, unique_specs: specifications.size }));
