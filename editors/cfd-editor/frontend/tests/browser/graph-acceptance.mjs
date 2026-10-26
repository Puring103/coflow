import { build, preview } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { tmpdir, cpus, totalmem } from 'node:os'
import { writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'

const root = fileURLToPath(new URL('../..', import.meta.url))
const outDir = join(tmpdir(), 'coflow-graph-acceptance-build')
const output = process.env.GRAPH_PERF_OUTPUT ?? join(tmpdir(), 'coflow-graph-acceptance.json')
const firstSamples = Number(process.env.GRAPH_PERF_FIRST_SAMPLES ?? 30)
const steadyMs = Number(process.env.GRAPH_PERF_STEADY_MS ?? 30000)
const rounds = Number(process.env.GRAPH_PERF_ROUNDS ?? 3)
const updates = Number(process.env.GRAPH_PERF_UPDATES ?? 1000)
const samples = [], steady = [], changes = []
const result = {
  date: new Date().toISOString(), ref: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
  diff: execFileSync('git', ['diff', '--stat'], { cwd: root, encoding: 'utf8' }).trim(),
  cpu: cpus()[0]?.model, logical_cpus: cpus().length, physical_memory_bytes: totalmem(), node: process.version,
  method: 'Serial production Chromium, 1440x900 DPR=1. navigation_ms includes module download and two RAFs. data_stable_ms starts at prepared GraphData before React render, ends after all height probes and two RAFs; excludes backend, transfer and ELK cold layout. Standard: editable controls, at most 300 drawn edges. Dense readonly matches previous fixture. Steady intervals include RAF wait and setViewport. Update timings include React and two RAFs, exclude backend IPC. GC heap excludes native/browser allocations.',
  protocol: { firstSamples, steadyMs, rounds, updates }, samples, steady, changes,
}
const save = () => writeFileSync(output, JSON.stringify(result, null, 2) + '\n')
await build({ root, logLevel: 'error', build: { outDir, emptyOutDir: true,
  rollupOptions: { input: join(root, 'tests/browser/graph-performance.html') } } })
const server = await preview({ root, logLevel: 'error', build: { outDir }, preview: { host: '127.0.0.1', port: 0 } })
const browser = await chromium.launch({ headless: true })
result.chromium = browser.version()
const settle = page => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
async function open(query) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  await page.addInitScript(() => {
    window.perfLongTasks = []
    new PerformanceObserver(list => window.perfLongTasks.push(...list.getEntries().map(entry => ({ start: entry.startTime, duration: entry.duration })))).observe({ type: 'longtask', buffered: true })
  })
  const start = performance.now()
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/graph-performance.html?${query}`, { timeout: 180000 })
  await page.locator('.graph-node').first().waitFor({ timeout: 180000 })
  await settle(page)
  const navigation_ms = performance.now() - start
  await page.waitForFunction(() => window.graphStableAt !== undefined, null, { timeout: 180000 })
  await settle(page)
  const metrics = await page.evaluate(() => ({
    data_stable_ms: window.graphStableAt - window.graphDataReadyAt,
    elements: document.querySelectorAll('*').length, rows: document.querySelectorAll('.dc-row').length,
    handles: document.querySelectorAll('.react-flow__handle').length, edges: document.querySelectorAll('.react-flow__edge').length,
    logical_rows: window.graphFlow.getNodes().reduce((count, node) => count + node.data.geometry.rows.length, 0),
    height: window.graphFlow.getNode('Item::source').data.geometry.height,
    heap_bytes: performance.memory?.usedJSHeapSize ?? null, long_tasks: window.perfLongTasks,
  }))
  return { page, sample: { query, navigation_ms, ...metrics, errors } }
}
try {
  for (let sample = 0; sample < firstSamples; sample++) {
    const current = await open('items=10000&standard&editable')
    samples.push({ group: 'standard-first', sample, ...current.sample })
    await current.page.close()
    if ((sample + 1) % 10 === 0) { console.log(`first ${sample + 1}/${firstSamples}`); save() }
  }
  for (let round = 0; round < rounds; round++) {
    const current = await open('items=10000&standard&editable')
    const frames = await current.page.evaluate(async duration => {
      window.perfLongTasks.length = 0
      const intervals = [], counts = []
      const start = performance.now()
      let previous = start
      const geometry = window.graphFlow.getNode('Item::source').data.geometry
      while (performance.now() - start < duration) {
        const now = await new Promise(requestAnimationFrame)
        intervals.push(now - previous); previous = now
        const phase = (now - start) / 2500
        void window.graphFlow.setViewport({ x: 100, y: -100 - (0.5 + 0.45 * Math.sin(phase)) * geometry.height, zoom: 1 + 0.2 * Math.sin(phase * 0.7) })
        if (intervals.length % 60 === 0) counts.push({ rows: document.querySelectorAll('.dc-row').length, edges: document.querySelectorAll('.react-flow__edge').length })
      }
      await new Promise(requestAnimationFrame)
      return { intervals_ms: intervals, counts, long_tasks: window.perfLongTasks }
    }, steadyMs)
    steady.push({ round, ...frames, errors: current.sample.errors })
    await current.page.close(); console.log(`steady ${round + 1}/${rounds}`); save()
  }
  // 密集连线与 100k 压力单列，不能混入标准场景的 P95。
  if (process.env.GRAPH_PERF_CAPACITY !== '0') for (let round = 0; round < rounds; round++) for (const query of [
    'items=1000&standard&editable', 'items=10000', 'items=10000&compact',
    'items=10000&kind=scalar&standard&editable', 'items=10000&kind=objects&standard&editable',
    'items=10000&kind=dict&standard&editable', 'items=10000&kind=strings&standard',
    'items=100000&standard&editable',
  ]) {
    const current = await open(query)
    if (query.includes('kind=')) {
      await current.page.locator('.graph-node[data-nodeid="Item::source"] .dc-row[data-field-path="values"]').click()
      await current.page.waitForFunction(() => window.graphFlow.getNode('Item::source').data.geometry.pendingCount === 0)
      await settle(current.page)
      current.sample.expanded = await current.page.evaluate(() => ({ rows: document.querySelectorAll('.dc-row').length, height: window.graphFlow.getNode('Item::source').data.geometry.height, heap_bytes: performance.memory?.usedJSHeapSize }))
    }
    samples.push({ group: 'capacity', round, ...current.sample })
    await current.page.close(); console.log(`capacity ${round} ${query}`); save()
  }
  const current = await open('items=10000&standard&editable')
  const cdp = await current.page.context().newCDPSession(current.page)
  // React 的当前/备用树在首轮修改后各保留一个代际；先预热再比较可回收堆。
  await current.page.evaluate(async () => {
    for (let i = 0; i < 20; i++) {
      window.graphHarness.setData(previous => ({ ...previous, revision: previous.revision + 1, nodes: previous.nodes.map(node => node.coordinate.id !== 'source' ? node : {
        ...node, fields: node.fields.map(field => field.name !== 'name' ? field : { ...field, value: { kind: 'string', value: `Warmup ${i}` } }),
      }) }))
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
    }
  })
  await cdp.send('HeapProfiler.collectGarbage')
  const heapBefore = await cdp.send('Runtime.getHeapUsage')
  const timings = await current.page.evaluate(async updates => {
    const timings = [], heaps = []
    for (let i = 0; i < updates; i++) {
      const start = performance.now()
      window.graphHarness.setData(previous => ({ ...previous, revision: previous.revision + 1, nodes: previous.nodes.map(node => node.coordinate.id !== 'source' ? node : {
        ...node, fields: node.fields.map(field => field.name !== 'name' ? field : { ...field, value: { kind: 'string', value: `Changed ${i}` } }),
      }) }))
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
      timings.push(performance.now() - start)
      if (i % 100 === 0) heaps.push(performance.memory?.usedJSHeapSize ?? null)
      if (i % 25 === 0) { window.graphHarness.setCompact((i / 25) % 2 === 0); await new Promise(requestAnimationFrame) }
    }
    return { timings_ms: timings, heaps_bytes: heaps, rows: document.querySelectorAll('.dc-row').length }
  }, updates)
  await cdp.send('HeapProfiler.collectGarbage')
  changes.push({ updates, ...timings, gc_heap_before: heapBefore, gc_heap_after: await cdp.send('Runtime.getHeapUsage'), errors: current.sample.errors })
  await current.page.evaluate(() => window.graphHarness.setGraphVisible(false))
  await settle(current.page)
  await cdp.send('HeapProfiler.collectGarbage')
  changes.at(-1).gc_heap_unmounted = await cdp.send('Runtime.getHeapUsage')
  await current.page.close(); console.log(`updates ${updates}`); save()
  const lifecycle = await open('items=100&editable')
  const lifecycleCdp = await lifecycle.page.context().newCDPSession(lifecycle.page)
  const heaps = []
  for (let cycle = 0; cycle < 22; cycle++) {
    await lifecycle.page.evaluate(() => window.graphHarness.setGraphVisible(false))
    await settle(lifecycle.page)
    await lifecycle.page.evaluate(() => window.graphHarness.setGraphVisible(true))
    await lifecycle.page.waitForFunction(() => window.graphFlow.getNodes().length > 0 && window.graphFlow.getNodes().every(node => node.data.geometry.pendingCount === 0))
    await settle(lifecycle.page)
    if ([1, 11, 21].includes(cycle)) {
      await lifecycleCdp.send('HeapProfiler.collectGarbage')
      heaps.push({ cycle: cycle + 1, ...await lifecycleCdp.send('Runtime.getHeapUsage') })
    }
  }
  result.lifecycle = { items: 100, warmups: 2, cycles: 20, heaps, errors: lifecycle.sample.errors }
  await lifecycle.page.close(); console.log('view lifecycle 20'); save()
} finally { save(); await browser.close(); await server.httpServer.close() }
