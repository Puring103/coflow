// 仅分析现有 JSON；所有负例只写入本目录的证据副本，不启动被测程序。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const repo = process.cwd();
const archive = path.join(repo, 'docs/archive/benchmarks/non-editor-optimization-2026-10-29/final-results');
const work = path.join(repo, 'target/performance-worklogs/analysis-regression');
const tool = path.join(repo, 'scripts/performance/report-optimization-targets.mjs');
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const write = (file, data) => fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n');
const digest = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const ids = [
  'rust-node-budget',
  'rust-sparse-dimension_existing_set', 'rust-sparse-dimension_existing_clear',
  'rust-dense-dimension_existing_set', 'rust-dense-dimension_existing_clear',
];
const summary = read(path.join(archive, 'summary.json'));
const comparison = read(path.join(archive, 'comparison.json'));
const previous = read(path.join(archive, 'target-status.json'));
const originalPaths = [...new Set([
  ...summary.rows.map(row => path.resolve(archive, row.source)),
  path.join(archive, 'summary.json'), path.join(archive, 'comparison.json'),
  path.join(repo, 'docs/archive/benchmarks/non-editor-2026-10-28/README.md'),
])].sort();
const fingerprints = originalPaths.map(file => ({ path: file, sha256: digest(file) }));
fs.mkdirSync(work, { recursive: true });

function run(input, output) {
  const result = spawnSync(process.execPath, [tool, input, '--output', output], { encoding: 'utf8', windowsHide: true });
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'analysis.stdout.log'), result.stdout ?? '');
  fs.writeFileSync(path.join(output, 'analysis.stderr.log'), result.stderr ?? '');
  assert.equal(result.status, 0, `Report failed: ${result.stderr}`);
  return read(path.join(output, 'target-status.json'));
}
function target(report, id) {
  const matches = report.targets.filter(item => item.id === id);
  assert.equal(matches.length, 1);
  return matches[0];
}
function assertPass(report, id) {
  const item = target(report, id);
  assert.equal(item.status, 'passed', id);
  assert.equal(item.criteria.length, 1);
  assert.equal(item.criteria[0].optimized_processes, 3, id);
  assert.equal(item.criteria[0].paired_rounds, 3, id);
  assert.equal(item.criteria[0].excluded_observations.length, 0, id);
  assert.ok(item.evidence.every(row => row.problems.length === 0), id);
}

const clean = run(archive, path.join(work, 'progress-events-valid'));
ids.forEach(id => assertPass(clean, id));
// 原预算及其余目标保持一致，变化只来自恢复这五项有效证据。
for (const old of previous.targets) {
  const current = target(clean, old.id);
  assert.deepEqual(current.criteria.map(({ key, kind, limit, unit }) => ({ key, kind, limit, unit })),
    old.criteria.map(({ key, kind, limit, unit }) => ({ key, kind, limit, unit })));
  if (!ids.includes(old.id)) assert.equal(current.status, old.status, old.id);
}

const scenarios = new Set(ids.map(id => target(clean, id).scenario));
const subsetRows = summary.rows.filter(row => scenarios.has(row.run.replace(/-r\d+-(baseline|optimized)$/u, '')));
const subsetSources = [...new Set(subsetRows.map(row => row.source))];
assert.equal(subsetSources.length, 18);
function copyEvidence(name) {
  const input = path.join(work, name, 'input');
  fs.mkdirSync(input, { recursive: true });
  write(path.join(input, 'summary.json'), {
    ...summary,
    rows: subsetRows,
    runs: summary.runs.filter(row => scenarios.has(row.spec?.scenario)),
    failures: (summary.failures ?? []).filter(row => subsetSources.includes(row.source)),
  });
  write(path.join(input, 'comparison.json'), {
    ...comparison,
    comparisons: (comparison.comparisons ?? []).filter(row => scenarios.has(row.scenario)),
  });
  for (const source of subsetSources) {
    const destination = path.resolve(input, source);
    assert.ok(destination.startsWith(input + path.sep));
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.resolve(archive, source), destination);
  }
  return input;
}
const sourceName = target(clean, 'rust-node-budget').evidence.find(row => row.arm === 'optimized' && row.round === 1).source;
const originalLines = fs.readFileSync(path.resolve(archive, sourceName), 'utf8').trimEnd().split(/\r?\n/u);
const aggregateLine = originalLines.find(line => {
  const entry = JSON.parse(line);
  return entry.metric === 'load_budget_rejected' && Array.isArray(entry.samples_ms);
});
assert.ok(aggregateLine);

function assertRejected(report, problem) {
  const item = target(report, 'rust-node-budget');
  assert.equal(item.status, 'insufficient_evidence');
  assert.equal(item.criteria[0].optimized_processes, 2);
  assert.equal(item.criteria[0].paired_rounds, 2);
  assert.ok(item.criteria[0].rejected_sources.some(row => row.source === sourceName && row.problems.includes(problem)));
  ids.filter(id => id !== 'rust-node-budget').forEach(id => assertPass(report, id));
  return { status: item.status, optimized_processes: item.criteria[0].optimized_processes, paired_rounds: item.criteria[0].paired_rounds, rejected_sources: item.criteria[0].rejected_sources };
}

const duplicateInput = copyEvidence('real-aggregate-duplicate');
fs.appendFileSync(path.join(duplicateInput, sourceName), '\n' + aggregateLine + '\n');
const duplicate = assertRejected(run(duplicateInput, path.join(work, 'real-aggregate-duplicate', 'report')), 'duplicate_raw_metric');

// 未登记的事件不能被宽泛地忽略，否则异常指标可能被静默丢弃。
const unknownInput = copyEvidence('unknown-event');
fs.appendFileSync(path.join(unknownInput, sourceName), '\n' + JSON.stringify({ event: 'unrecognized_progress_event', metric: 'load_budget_rejected' }) + '\n');
const unknown = assertRejected(run(unknownInput, path.join(work, 'unknown-event', 'report')), 'duplicate_raw_metric');

// 缺失原始样本的聚合记录仍留在验证链中，不能因缺少 samples_ms 被过滤。
const missingInput = copyEvidence('missing-aggregate-samples');
const malformed = JSON.parse(aggregateLine);
delete malformed.samples_ms;
fs.writeFileSync(path.join(missingInput, sourceName), originalLines.map(line => line === aggregateLine ? JSON.stringify(malformed) : line).join('\n') + '\n');
const missing = assertRejected(run(missingInput, path.join(work, 'missing-aggregate-samples', 'report')), 'invalid_raw_samples');

for (const item of fingerprints) assert.equal(digest(item.path), item.sha256, `Original evidence changed: ${item.path}`);
const result = {
  success: true,
  scope: 'Existing JSON analysis only; no measured executable, build or benchmark was run.',
  tool: { path: tool, sha256: digest(tool) },
  original_evidence_unchanged: fingerprints,
  five_targets: ids.map(id => {
    const item = target(clean, id), criterion = item.criteria[0];
    return { id, status: item.status, observed: criterion.observed, limit: criterion.limit, optimized_processes: criterion.optimized_processes, paired_rounds: criterion.paired_rounds };
  }),
  clean_counts: clean.counts,
  clean_peak_counts: clean.peak_counts,
  negative_cases: { real_aggregate_duplicate: duplicate, unknown_event: unknown, missing_aggregate_samples: missing },
};
write(path.join(work, 'progress-event-regression.json'), result);
console.log(JSON.stringify({ success: result.success, five_targets: result.five_targets, negative_cases: result.negative_cases, original_files_verified_unchanged: fingerprints.length, clean_counts: clean.counts, clean_peak_counts: clean.peak_counts }));
