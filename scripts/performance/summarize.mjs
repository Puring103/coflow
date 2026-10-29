import fs from 'node:fs';
import path from 'node:path';

const directory = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-2026-10-28');
// 只读取根目录和显式指定的目录；不递归收集烟测或历史试验。
// 用法：node summarize.mjs <归档根> retry-cli [其他归档子目录 ...]
const inputDirectories = [...new Set([directory, ...process.argv.slice(3).map(value => path.resolve(directory, value))]
  .map(value => fs.realpathSync(value)))];
const files = new Map();
for (const inputDirectory of inputDirectories) {
  for (const name of fs.readdirSync(inputDirectory).sort()) {
    const absolutePath = path.join(inputDirectory, name);
    if (!fs.statSync(absolutePath).isFile()) continue;
    const identity = fs.realpathSync(absolutePath);
    // 同一路径的重复参数或链接只收录一次；相同文件名的重试仍保留各自证据。
    if (!files.has(identity)) files.set(identity, { absolutePath, file: path.relative(directory, absolutePath).split(path.sep).join('/') });
  }
}
const rows = [];
const runs = [];
const failures = [];
const median = xs => xs.length % 2 ? xs[(xs.length - 1) / 2] : (xs[xs.length / 2 - 1] + xs[xs.length / 2]) / 2;

// 所有工具统一按原始样本重算，避免不同工具的偶数中位数口径混用。
function collect(entry, file, context = {}) {
  if (!entry || typeof entry !== 'object') return;
  if (Array.isArray(entry)) { entry.forEach(item => collect(item, file, context)); return; }
  if (entry.metrics) collect(entry.metrics, file, { ...context, ...(entry.config ?? {}) });
  if (entry.correctness && entry.correctness !== 'passed') return;
  if (!Array.isArray(entry.samples_ms) || !entry.samples_ms.length) return;
  if (!entry.samples_ms.every(x => Number.isFinite(x) && x >= 0)) throw new Error(`Invalid samples: ${file}`);
  const sorted = [...entry.samples_ms].sort((a, b) => a - b);
  const count = sorted.length;
  const operations = entry.operations_per_sample ?? (/^model_clone_10000_f/u.test(entry.metric ?? '') ? 10000 : 1);
  rows.push({
    run: file.replace(/\.stdout\.ndjson$|\.results\.json$|\.json$/u, ''),
    metric: entry.metric ?? entry.name ?? entry.case,
    samples: count,
    median_ms: median(sorted),
    min_ms: sorted[0],
    max_ms: sorted[count - 1],
    p95_ms: count >= 20 ? sorted[Math.ceil(count * .95) - 1] : null,
    p99_ms: count >= 100 ? sorted[Math.ceil(count * .99) - 1] : null,
    operations_per_sample: operations,
    median_per_operation_ms: median(sorted) / operations,
    source: file,
    records: entry.records ?? context.records ?? context.Records ?? null,
    files: entry.files ?? context.files ?? context.Files ?? null,
    fields: entry.fields ?? context.fields ?? context.Fields ?? null,
    source_bytes: entry.source_bytes ?? context.source_bytes ?? context.utf8_bytes ?? null,
    median_allocated_bytes: entry.samples_detail?.[0]?.AllocatedBytes != null ? median(entry.samples_detail.map(item => item.AllocatedBytes).sort((a, b) => a - b)) : null,
    median_allocated_current_thread_bytes: entry.samples_detail?.[0]?.allocated_current_thread_bytes != null ? median(entry.samples_detail.map(item => item.allocated_current_thread_bytes).sort((a, b) => a - b)) : null,
    median_retained_delta_bytes: entry.samples_detail?.[0]?.RetainedDeltaBytes != null ? median(entry.samples_detail.map(item => item.RetainedDeltaBytes).sort((a, b) => a - b)) : entry.samples_detail?.[0]?.retained_delta_bytes != null ? median(entry.samples_detail.map(item => item.retained_delta_bytes).sort((a, b) => a - b)) : null,
  });
}

for (const { absolutePath, file } of [...files.values()].sort((a, b) => a.file.localeCompare(b.file))) {
  if (file.endsWith('.stdout.ndjson')) {
    // 未退出的进程可能仍在写最后一行，只汇总已保存执行状态的日志。
    const stem = file.replace(/\.stdout\.ndjson$/u, '');
    if (!fs.existsSync(path.join(directory, `${stem}.meta.json`)) && !fs.existsSync(path.join(directory, `${file}.process.json`))) continue;
    const content = fs.readFileSync(absolutePath, 'utf8');
    let whole;
    try { whole = JSON.parse(content); } catch { /* Rust/C# 逐行输出多个 JSON 文档。 */ }
    if (whole) collect(whole, file);
    else {
      let context = {};
      const entries = content.split(/\r?\n/u).filter(line => line.trim().startsWith('{')).map(line => JSON.parse(line));
      // 旧 engine_perf 在计时行之后输出 parser 规格；先建立关联，避免继承前一维度夹具的规模。
      const parserSpecifications = new Map(entries.filter(entry => entry.parse_records != null)
        .map(entry => [`parse_${entry.parse_records}_broken_${entry.broken}`, entry]));
      for (const entry of entries) {
        context = { ...context, ...(entry.config ?? {}), ...Object.fromEntries(['records', 'files', 'fields', 'source_bytes', 'utf8_bytes'].filter(key => entry[key] != null).map(key => [key, entry[key]])) };
        const parser = parserSpecifications.get(entry.metric);
        collect(entry, file, parser ? { records: parser.parse_records, files: 1, fields: 1, source_bytes: parser.bytes } : context);
      }
    }
  } else if (file.endsWith('.results.json')) {
    const report = JSON.parse(fs.readFileSync(absolutePath, 'utf8').replace(/^\uFEFF/u, ''));
    collect(report, file);
    for (const metric of report.metrics ?? []) if (metric.correctness !== 'passed') {
      failures.push({ source: file, metric: metric.metric, status: metric.correctness,
        collected_samples: metric.samples_ms?.length ?? 0, error: metric.error ?? null, failure: metric.failure ?? null });
    }
  } else if (file.endsWith('.meta.json') || file.endsWith('.process.json')) {
    runs.push({ source: file, ...JSON.parse(fs.readFileSync(absolutePath, 'utf8').replace(/^\uFEFF/u, '')) });
  }
}

fs.writeFileSync(path.join(directory, 'summary.json'), JSON.stringify({
  input_directories: inputDirectories.map(value => path.relative(directory, value).split(path.sep).join('/') || '.'),
  quantile_method: 'Arithmetic median; nearest-rank observed P95 for n>=20, P99 for n>=100. These are descriptive sample quantiles, not stable tail guarantees.',
  rows,
  runs,
  failures,
}, null, 2) + '\n');
const columns = ['run', 'metric', 'records', 'files', 'fields', 'source_bytes', 'samples', 'median_ms', 'min_ms', 'max_ms', 'p95_ms', 'p99_ms', 'operations_per_sample', 'median_per_operation_ms', 'median_allocated_bytes', 'median_allocated_current_thread_bytes', 'median_retained_delta_bytes', 'source'];
const csv = value => value == null ? '' : `"${String(value).replaceAll('"', '""')}"`;
fs.writeFileSync(path.join(directory, 'summary.csv'), columns.join(',') + '\n' + rows.map(row => columns.map(key => csv(row[key])).join(',')).join('\n') + '\n');
const format = x => x == null ? '—' : x.toFixed(3);
fs.writeFileSync(path.join(directory, 'measurements.md'), '# 原始样本汇总\n\n耗时单位为毫秒。每次样本包含的操作数量见 `每样本操作数`；批量查询的单操作均值不能当作单次调用尾延迟。少于 20 个样本不报告 P95，少于 100 个不报告 P99；已列分位数也仅描述本次样本。\n\n| 运行 | 指标 | 样本数 | 中位数 | 最小值 | 最大值 | P95 | 每样本操作数 |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n' + rows.map(row => `| ${row.run} | ${row.metric} | ${row.samples} | ${format(row.median_ms)} | ${format(row.min_ms)} | ${format(row.max_ms)} | ${format(row.p95_ms)} | ${row.operations_per_sample} |`).join('\n') + '\n');
console.log(JSON.stringify({ rows: rows.length, directory }));
