import { build, preview } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { tmpdir, cpus } from 'node:os'
import { writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'

const root = fileURLToPath(new URL('../..', import.meta.url))
const outDir = join(tmpdir(), 'coflow-graph-perf-build')
const items = (process.env.GRAPH_PERF_ITEMS ?? '100,1000,10000').split(',').map(Number)
const rounds = Number(process.env.GRAPH_PERF_ROUNDS ?? 3)
await build({ root, logLevel: 'error', build: { outDir, emptyOutDir: true,
  rollupOptions: { input: join(root, 'tests/browser/graph-performance.html') } } })
const server = await preview({ root, logLevel: 'error', build: { outDir }, preview: { host: '127.0.0.1', port: 0 } })
const browser = await chromium.launch({ headless: true })
const samples = []
try {
  for (let round = 0; round < rounds; round++) for (const count of items) for (const compact of [false, true]) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
    const errors = []
    page.on('pageerror', error => errors.push(error.message))
    const start = performance.now()
    await page.goto(`${server.resolvedUrls.local[0]}tests/browser/graph-performance.html?items=${count}${compact ? '&compact' : ''}`)
    await page.locator('.graph-node').first().waitFor({ timeout: 120000 })
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
    const mounted_ms = performance.now() - start
    const metrics = await page.evaluate(() => ({
      elements: document.querySelectorAll('*').length,
      rows: document.querySelectorAll('.dc-row').length,
      handles: document.querySelectorAll('.react-flow__handle').length,
      edges: document.querySelectorAll('.react-flow__edge').length,
      heap_bytes: performance.memory?.usedJSHeapSize ?? null,
    }))
    samples.push({ round, items: count, compact, mounted_ms, ...metrics, errors })
    console.log(JSON.stringify(samples.at(-1)))
    await page.close()
  }
} finally { await browser.close(); await server.httpServer.close() }
writeFileSync(process.env.GRAPH_PERF_OUTPUT ?? join(tmpdir(), 'coflow-graph-performance.json'), JSON.stringify({
  date: new Date().toISOString(), ref: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
  cpu: cpus()[0]?.model, node: process.version,
  method: 'Production browser fixture; mounted_ms includes navigation and two animation frames, not backend or ELK cold layout. Samples below 30 do not establish P95.',
  samples,
}, null, 2) + '\n')
