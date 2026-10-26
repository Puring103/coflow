import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'

const directory = fileURLToPath(new URL('../docs/archive/benchmarks/', import.meta.url))
const read = file => JSON.parse(readFileSync(join(directory, file), 'utf8'))
const raw = file => readFileSync(join(directory, file), 'utf8').trim().split(/\r?\n/).map(JSON.parse)
function quantiles(values) {
  // 中位数取中间两项平均；短样本只保留范围，避免把单次尖峰包装为稳定分位数。
  const sorted = [...values].sort((a, b) => a - b)
  const count = sorted.length
  const at = p => sorted[Math.ceil(count * p) - 1]
  return { samples: count, p50_ms: count % 2 ? sorted[(count - 1) / 2] : (sorted[count / 2 - 1] + sorted[count / 2]) / 2,
    p95_ms: count >= 20 ? at(.95) : null, p99_ms: count >= 100 ? at(.99) : null, min_ms: sorted[0], max_ms: sorted.at(-1) }
}

const runs = []
for (const kind of ['acceptance', 'cold', 'scaling']) {
  const metadata = read(`incremental-graph-engine-${kind}-2026-10-26.json`)
  for (const run of metadata.runs) {
    const rows = raw(run.raw)
    const config = rows.find(row => row.records)
    runs.push({ ...run, files: config.files,
      metrics: rows.filter(row => row.samples_ms).map(row => ({ metric: row.metric,
        ...quantiles(row.samples_ms.length === 219 ? row.samples_ms.slice(19) : row.samples_ms),
        warmups: row.samples_ms.length === 219 ? 20 : 1 })),
      stats: rows.filter(row => row.stats), verified: rows.some(row => row.verification === 'fresh_reload_matches_all_values_and_no_diagnostics') })
  }
}
const pooled = {}
for (const label of ['baseline', 'current']) {
  const values = runs.filter(run => run.label === label && run.records === 100000 && run.batches === '1' && run.round < 3)
    .flatMap(run => raw(run.raw).find(row => row.metric === 'mutate_1_grouped').samples_ms.slice(19))
  pooled[label] = quantiles(values)
}
writeFileSync(join(directory, 'incremental-graph-engine-summary-2026-10-26.json'), JSON.stringify({
  method: 'P50 is the arithmetic median; P95/P99 use nearest rank. Raw 219 mutation samples discard first 19; example warmup 1 gives 20 warmups and 200 retained samples. Three-sample batches report median/range only. Fresh-session cold data opens share process, filesystem and code caches. External process working set includes all memory, not just model allocations.',
  runs, pooled,
}, null, 2) + '\n')

if (existsSync(join(directory, 'large-graph-acceptance-2026-10-26.json'))) {
  const graph = read('large-graph-acceptance-2026-10-26.json')
  const first = graph.samples.filter(sample => sample.group === 'standard-first')
  const summary = { first: quantiles(first.map(sample => sample.data_stable_ms)),
    navigation: quantiles(first.map(sample => sample.navigation_ms)),
    steady: graph.steady.map(round => ({ round: round.round, ...quantiles(round.intervals_ms),
      long_tasks: round.long_tasks.length, max_long_task_ms: Math.max(0, ...round.long_tasks.map(task => task.duration)),
      max_rows: Math.max(0, ...round.counts.map(count => count.rows)), max_edges: Math.max(0, ...round.counts.map(count => count.edges)) })),
    updates: graph.changes.map(change => ({ ...quantiles(change.timings_ms), gc_heap_before: change.gc_heap_before,
      gc_heap_after: change.gc_heap_after, errors: change.errors })), lifecycle: graph.lifecycle,
    errors: [...graph.samples, ...graph.steady, ...graph.changes].flatMap(sample => sample.errors ?? []) }
  writeFileSync(join(directory, 'large-graph-summary-2026-10-26.json'), JSON.stringify(summary, null, 2) + '\n')
  console.log(JSON.stringify({ engine: pooled, graph: summary }, null, 2))
} else console.log(JSON.stringify({ engine: pooled, runs: runs.length }, null, 2))
