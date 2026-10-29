import { build, preview } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'
import { join, resolve } from 'node:path'
import { tmpdir, cpus } from 'node:os'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
const root = fileURLToPath(new URL('../..', import.meta.url))
const ref = process.env.GRAPH_PERF_BASELINE_REF ?? '3742bf1e9dc8d3a22a263f1db48be32412b1b3a4'
const output = process.env.GRAPH_PERF_OUTPUT ?? join(tmpdir(), 'coflow-graph-paired.json')
const samples = Number(process.env.GRAPH_PERF_FIRST_SAMPLES ?? 20)
const paths = ['src/features/graph/components/GraphView.tsx', 'src/features/graph/components/GraphNode.tsx', 'src/features/graph/components/GraphView.state.ts']
const servers = {}, sources = {}
const result = { date: new Date().toISOString(), ref, cpu: cpus()[0].model, node: process.version, method: 'Single Chromium process, alternating baseline/current order for each pair; production builds, unchanged fixture and four-RAF stable predicate, 1440x900 DPR=1. No profiler or diagnostic instrumentation.', source_sha256: sources, samples: [] }
for (const label of ['baseline', 'current']) {
  const overrides = new Map()
  sources[label] = Object.fromEntries(paths.map(path => {
    const source = label === 'baseline' ? execFileSync('git', ['show', `${ref}:editors/cfd-editor/frontend/${path}`], { cwd: root, encoding: 'utf8' }) : readFileSync(join(root, path), 'utf8')
    if (label === 'baseline') overrides.set(resolve(root, path).replaceAll('\\','/').toLowerCase(), source)
    return [path, createHash('sha256').update(source).digest('hex')]
  }))
  const outDir = join(tmpdir(), `coflow-graph-paired-${label}`)
  await build({ root, logLevel: 'error', plugins: [{ name: 'paired-baseline', enforce: 'pre', load(id) { return overrides.get(id.replaceAll('\\','/').toLowerCase()) } }], build: { outDir, emptyOutDir: true, rollupOptions: { input: join(root, 'tests/browser/graph-performance.html') } } })
  servers[label] = await preview({ root, logLevel: 'error', build: { outDir }, preview: { port: 0, host: '127.0.0.1' } })
}
const browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH })
result.chromium = browser.version()
const save = () => writeFileSync(output, JSON.stringify(result, null, 2)+'\n')
try {
  for (let pair = 0; pair < samples; pair++) for (const label of pair % 2 ? ['current', 'baseline'] : ['baseline', 'current']) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } }), errors = []
    page.on('pageerror', error => errors.push(error.message))
    await page.goto(`${servers[label].resolvedUrls.local[0]}tests/browser/graph-performance.html?items=10000&standard&editable`)
    await page.waitForFunction(() => window.graphStableAt !== undefined)
    const metrics = await page.evaluate(() => ({ data_stable_ms: window.graphStableAt-window.graphDataReadyAt, rows: document.querySelectorAll('.dc-row').length, edges: document.querySelectorAll('.react-flow__edge').length }))
    result.samples.push({ pair, label, ...metrics, errors })
    await page.close(); save()
    if (label === (pair % 2 ? 'baseline' : 'current') && (pair+1)%5===0) console.log(`pairs ${pair+1}/${samples}`)
  }
} finally { save(); await browser.close(); for (const server of Object.values(servers)) await new Promise(resolve => server.httpServer.close(resolve)) }
