// 比较同夹具独立进程的成对样本，超时不填为实际耗时。
import fs from 'node:fs';
import path from 'node:path';

const directory = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29');
const summary = JSON.parse(fs.readFileSync(path.join(directory, 'summary.json'), 'utf8'));
const groups = new Map();
const median = values => {
  if (!values.length) return null;
  const xs = [...values].sort((a, b) => a - b), n = xs.length;
  return n % 2 ? xs[(n - 1) / 2] : (xs[n / 2 - 1] + xs[n / 2]) / 2;
};
for (const row of summary.rows) {
  const match = /^(.*)-r(\d+)-(baseline|optimized)$/u.exec(row.run);
  if (!match) continue;
  const [, scenario, round, arm] = match;
  const identity = JSON.stringify([scenario, row.metric, row.records, row.fields, row.operations_per_sample]);
  if (!groups.has(identity)) groups.set(identity, { scenario, metric: row.metric, records: row.records, fields: row.fields, operations_per_sample: row.operations_per_sample, baseline: [], optimized: [] });
  groups.get(identity)[arm].push({ round: Number(round), ...row });
}
const comparisons = [...groups.values()].map(group => {
  const baseline = median(group.baseline.map(row => row.median_ms));
  const optimized = median(group.optimized.map(row => row.median_ms));
  const pairs = group.baseline.flatMap(old => {
    const next = group.optimized.find(row => row.round === old.round);
    return next ? [{ round: old.round, baseline_ms: old.median_ms, optimized_ms: next.median_ms, ratio: next.median_ms / old.median_ms }] : [];
  });
  const memory = {};
  for (const key of ['median_allocated_bytes', 'median_allocated_current_thread_bytes', 'median_retained_delta_bytes']) {
    memory[key] = Object.fromEntries(['baseline', 'optimized'].map(arm => [arm, median(group[arm].map(row => row[key]).filter(value => value != null))]));
  }
  return { scenario: group.scenario, metric: group.metric, records: group.records, fields: group.fields, operations_per_sample: group.operations_per_sample,
    baseline_ms: baseline, optimized_ms: optimized, improvement_percent: baseline != null && baseline > 0 && optimized != null ? (1 - optimized / baseline) * 100 : null,
    paired_rounds: pairs.length, all_paired_rounds_faster: pairs.length >= 3 && pairs.every(pair => pair.ratio < 1), pairs, memory,
    evidence: [...group.baseline, ...group.optimized].map(row => ({ round: row.round, source: row.source, samples: row.samples, p95_ms: row.p95_ms, p99_ms: row.p99_ms })) };
});
const censored = summary.runs.filter(run => run.timed_out || run.exit_code !== 0).map(run => ({ source: run.source, timed_out: run.timed_out, exit_code: run.exit_code, timeout_seconds: run.timeout_seconds }));
fs.writeFileSync(path.join(directory, 'comparison.json'), JSON.stringify({ method: 'Arithmetic median of per-process medians. Paired ratios use the same scenario and round. Missing/failed/censored operations are never imputed. These are descriptive measurements, not statistical confidence intervals.', comparisons, censored, failures: summary.failures }, null, 2) + '\n');
const number = value => value == null ? '—' : value.toFixed(3);
fs.writeFileSync(path.join(directory, 'comparison.md'), '# 优化前后成对测量\n\n单位：毫秒。汇总值是各独立进程中位数的算术中位数；完整样本、每轮比例和内存指标见 comparison.json。超时不填作实际耗时。\n\n| 场景 | 操作 | 旧版 | 优化版 | 降低比例 | 配对轮数 |\n| --- | --- | ---: | ---: | ---: | ---: |\n' + comparisons.map(row => `| ${row.scenario} | ${row.metric} | ${number(row.baseline_ms)} | ${number(row.optimized_ms)} | ${number(row.improvement_percent)}% | ${row.paired_rounds} |`).join('\n') + '\n');
console.log(JSON.stringify({ comparisons: comparisons.length, censored: censored.length, failures: summary.failures.length }));
