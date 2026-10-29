import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

// 默认从仓库工作目录解析；显式参数可在任意目录指向完整归档。
assert.ok(process.argv.length <= 3, 'Usage: node audit-final-serial.mjs [archive-root]');
const archive = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29');
const groups = [
  ['final-results', 291], ['stability-r4', 6], ['cli-memory-r4', 24],
  ['calibration-r5', 18], ['calibration-r6', 14],
];
const files = directory => fs.readdirSync(directory, { withFileTypes: true }).flatMap(item => {
  const entry = path.join(directory, item.name);
  return item.isDirectory() ? files(entry) : item.name.endsWith('.meta.json') ? [entry] : [];
});
const runs = [];
for (const [directory, expected] of groups) {
  const entries = files(path.join(archive, directory));
  assert.equal(entries.length, expected, `Incomplete metadata: ${directory}`);
  for (const file of entries) {
    const bytes = fs.readFileSync(file);
    const meta = JSON.parse(bytes.toString('utf8').replace(/^\uFEFF/u, ''));
    const start = Date.parse(meta.started_at);
    assert.ok(Number.isFinite(start) && Number.isFinite(meta.wall_seconds));
    assert.ok(Number.isInteger(meta.exit_code) && typeof meta.timed_out === 'boolean');
    runs.push({ source: path.relative(archive, file).replaceAll('\\', '/'), id: meta.id,
      sha256: createHash('sha256').update(bytes).digest('hex'), start_ms: start,
      end_ms: start + meta.wall_seconds * 1000, exit_code: meta.exit_code, timed_out: meta.timed_out });
  }
}
runs.sort((a, b) => a.start_ms - b.start_ms);
const overlaps = [];
let previous = null;
for (const run of runs) {
  if (previous && run.start_ms < previous.end_ms - 2) overlaps.push({ first: previous.source, second: run.source, overlap_ms: previous.end_ms - run.start_ms });
  if (!previous || previous.end_ms < run.end_ms) previous = run;
}
const report = { audited_at: new Date().toISOString(), status: overlaps.length ? 'overlap_detected' : 'passed',
  method: 'Archived benchmark process start plus measured wall interval; final combined evidence, native stability, direct CLI memory and both managed calibrations. A 2ms rounding tolerance matches the per-suite audit. Does not prove absence of unrelated OS or user activity; build/smoke separation is additionally documented in build receipts and pause receipts.',
  groups: groups.map(([directory, expected]) => ({ directory, expected })), count: runs.length, overlaps, runs };
fs.writeFileSync(path.join(archive, 'verification/final-serial-audit.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ count: runs.length, status: report.status, overlaps }));
process.exitCode = overlaps.length ? 1 : 0;
