import { build, preview } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { tmpdir, cpus, totalmem } from 'node:os'
import { readFileSync, writeFileSync } from 'node:fs'

// 串行生产构建采样；计时轮与 CPU 采样轮分离，避免 profiler 干扰帧间隔。
const root = fileURLToPath(new URL('../..', import.meta.url))
const outDir = join(tmpdir(), 'coflow-complex-perf-build')
const output = process.env.COMPLEX_PERF_OUTPUT ?? join(tmpdir(), 'coflow-complex-performance.json')
const disableSummaryMeasurement = process.env.COMPLEX_PERF_NO_SUMMARY_MEASURE === '1'
await build({ root, logLevel: 'error', plugins: disableSummaryMeasurement ? [{
  name: 'diagnostic-summary-measurement', enforce: 'pre',
  load(id) {
    if (!id.replaceAll('\\', '/').endsWith('/src/features/record-editing/components/DataCard.tsx')) return
    const source = readFileSync(id, 'utf8')
    const marker = '    return observeSummary(root, total, (visible, prepared) => setLimit(current => visible === prepared ? Math.min(total, current + 8) : Math.min(current, Math.max(8, visible + 8))))'
    const normalized = source.replaceAll('\r\n', '\n')
    if (!normalized.includes(marker)) throw new Error('Summary measurement diagnostic marker missing')
    // 仅诊断构建跳过摘要宽度测量，不改动正式组件文件。
    return normalized.replace(marker, '    return')
  },
}] : [], build: { outDir, emptyOutDir: true, rollupOptions: { input: join(root, 'tests/browser/performance.html') } } })
const server = await preview({ root, logLevel: 'error', build: { outDir }, preview: { port: 0, host: '127.0.0.1' } })
const browser = await chromium.launch({ headless: true })
const scenarios = process.env.COMPLEX_PERF_SCENARIOS ? JSON.parse(process.env.COMPLEX_PERF_SCENARIOS) : [
  { name: 'scalar', complexCols: 0, items: 8, depth: 1 },
  { name: 'array-2', complexCols: 2, items: 8, depth: 1 },
  { name: 'array-12', complexCols: 12, items: 8, depth: 1 },
  { name: 'array-2-long', complexCols: 2, items: 32, depth: 1 },
  { name: 'array-2-nested', complexCols: 2, items: 8, depth: 2 },
  { name: 'object-12', complexCols: 12, items: 8, depth: 1, shape: 'object' },
  { name: 'array-12-visible', complexCols: 12, items: 8, depth: 1, complexViewport: true },
  { name: 'array-2-long-visible', complexCols: 2, items: 32, depth: 1, complexViewport: true },
  { name: 'object-12-visible', complexCols: 12, items: 8, depth: 1, shape: 'object', complexViewport: true },
]
const results = []
async function scroll(page, axis = 'vertical') {
  return page.evaluate(axis => new Promise(resolve => {
    const element = document.querySelector('.table-scroll'), frames = []
    let previous, started, index = 0
    const limit = axis === 'vertical' ? element.scrollHeight - element.clientHeight : element.scrollWidth - element.clientWidth
    function step(now) {
      if (previous !== undefined) frames.push(now - previous)
      previous = now
      started ??= now
      const offset = axis === 'vertical' ? (index * 120) % Math.max(1, limit) : (index % 60) / 59 * limit
      if (axis === 'vertical') element.scrollTop = offset
      else element.scrollLeft = offset
      if (++index < 90 && now - started < 8000) requestAnimationFrame(step)
      else resolve(frames.slice(4))
    }
    requestAnimationFrame(step)
  }), axis)
}
try {
  for (let round = 0; round < 2; round++) {
    for (const scenario of round ? [...scenarios].reverse() : scenarios) {
      console.log(JSON.stringify({ starting: scenario.name, round, disableSummaryMeasurement }))
      const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
      const errors = []
      page.on('pageerror', error => errors.push(error.message))
      const params = new URLSearchParams({ rows: '3000', cols: '20', ...Object.fromEntries(Object.entries(scenario).map(([key, value]) => [key, String(value)])) })
      await page.goto(`${server.resolvedUrls.local[0]}tests/browser/performance.html?${params}`)
      await page.locator('tr[data-coordinate-id]').first().waitFor()
      await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 500)))
      const cdp = await page.context().newCDPSession(page)
      await cdp.send('Performance.enable')
      const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(metric => [metric.name, metric.value]))
      const axes = {}
      for (const axis of ['vertical', 'horizontal']) {
        await page.locator('.table-scroll').evaluate((element, showComplex) => { element.scrollTop = 0; element.scrollLeft = showComplex ? element.scrollWidth : 0 }, axis === 'vertical' && !!scenario.complexViewport)
        const before = await metrics()
        const frames = (await scroll(page, axis)).sort((a, b) => a - b)
        const after = await metrics()
        axes[axis] = {
          p50: +frames[Math.floor(frames.length * .5)].toFixed(1), p95: +frames[Math.floor(frames.length * .95)].toFixed(1),
          over33: frames.filter(value => value > 33.4).length, samples: frames.length,
          ms: Object.fromEntries(['ScriptDuration', 'LayoutDuration', 'RecalcStyleDuration', 'TaskDuration'].map(key => [key, +((after[key] - before[key]) * 1000).toFixed(1)])),
          dom: await page.evaluate(() => ({ nodes: document.querySelectorAll('*').length, complexCells: document.querySelectorAll('td.complex-cell').length })),
        }
      }
      const dom = await page.evaluate(() => ({ nodes: document.querySelectorAll('*').length, rows: document.querySelectorAll('tr[data-coordinate-id]').length,
        complexCells: document.querySelectorAll('td.complex-cell').length, fields: document.querySelectorAll('.dc-row').length,
        textareas: document.querySelectorAll('textarea').length, overflowing: document.querySelectorAll('.cell-complex-preview.overflowing').length }))
      if (round === 1 && scenario.name === 'array-12') {
        await cdp.send('Profiler.enable'); await cdp.send('Profiler.start')
        await scroll(page)
        const { profile } = await cdp.send('Profiler.stop')
        writeFileSync(output.replace(/\.json$/, '.cpuprofile'), JSON.stringify(profile))
      }
      const result = { round, ...scenario, axes, dom, errors }
      results.push(result); console.log(JSON.stringify(result))
      await page.close()
    }
  }
} finally {
  writeFileSync(output, JSON.stringify({ date: new Date().toISOString(), disableSummaryMeasurement, cpu: cpus()[0].model, memoryGiB: Math.round(totalmem() / 2 ** 30), browser: browser.version(), rows: 3000, cols: 20, viewport: [1280, 800], table: [880, 480], results }, null, 2))
  await browser.close(); await new Promise(resolve => server.httpServer.close(resolve))
}
