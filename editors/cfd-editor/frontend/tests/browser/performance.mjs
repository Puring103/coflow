import { build, preview } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { execFileSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
const root = fileURLToPath(new URL('../..', import.meta.url))
const baselineRef = process.env.PERF_BASELINE_REF ?? 'ac18f330'
const baseline = process.argv.includes('--baseline')
const outDir = join(tmpdir(), `coflow-perf-${baseline ? 'baseline' : 'current'}`)
// 基线使用已提交版本源码；两边共享数据、视窗、生产构建和采样流程。
const overrides = new Map()
if (baseline) {
  const paths = execFileSync('git', ['diff', '--name-only', baselineRef, '--', 'editors/cfd-editor/frontend/src'], { cwd: join(root, '../../..'), encoding: 'utf8' }).trim().split('\n')
  for (const path of paths.filter(Boolean)) {
    overrides.set(resolve(root, '../../..', path).replaceAll('\\', '/').toLowerCase(), execFileSync('git', ['show', `${baselineRef}:${path}`], { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }))
  }
}
let overridesUsed = 0
await build({ root, logLevel: 'error', plugins: [{ name: 'committed-baseline', enforce: 'pre', load(id) { const source = overrides.get(id.replaceAll('\\', '/').toLowerCase()); if (source !== undefined) overridesUsed++; return source } }], build: { outDir, emptyOutDir: true, rollupOptions: { input: join(root, 'tests/browser/performance.html') } } })
if (baseline && overridesUsed === 0) throw new Error('Baseline source override did not run')
console.log(JSON.stringify({ baseline, overridesUsed }))
const server = await preview({ root, logLevel: 'error', build: { outDir }, preview: { port: 0, host: '127.0.0.1' } })
const browser = await chromium.launch({ headless: true })
const results = []
try {
  for (const [rows, cols, inspector] of (process.argv.includes('--profile') ? [[10000,100,true]] : [[1000,6,false],[10000,6,false],[1000,60,false],[3000,60,true],[10000,100,true]])) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
    const errors = []
    page.on('pageerror', error => { errors.push(error.message); console.error(error.message) })
    const start = performance.now()
    await page.goto(`${server.resolvedUrls.local[0]}tests/browser/performance.html?rows=${rows}&cols=${cols}${inspector ? '&inspector' : ''}`)
    await page.locator('.cell-string-textarea').first().waitFor()
    const readyMs = Math.round(performance.now() - start)
    const cdp = await page.context().newCDPSession(page)
    await cdp.send('Performance.enable')
    const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(m => [m.name,m.value]))
    const before = await metrics()
    const frames = await page.evaluate(() => new Promise(resolve => {
      const scroller = document.querySelector('.table-scroll'), times = []
      let previous = performance.now(), i = 0
      function step(now) { times.push(now - previous); previous = now; scroller.scrollTop = i++ * 120; if(i < 90) requestAnimationFrame(step); else resolve(times.slice(2)) }
      requestAnimationFrame(step)
    }))
    const after = await metrics()
    await page.locator('.table-scroll').evaluate(el => { el.scrollTop = 0 })
    await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))
    await page.locator('.cell-string-textarea').first().click()
    if (process.argv.includes('--profile')) { await cdp.send('Profiler.enable'); await cdp.send('Profiler.start') }
    const selectAllMs = []
    for (let i=0;i<3;i++) {
      await page.keyboard.press('ArrowRight')
      const start = performance.now()
      await page.keyboard.press('Control+a')
      await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))
      selectAllMs.push(Math.round(performance.now()-start))
    }
    if (process.argv.includes('--profile')) { const { profile } = await cdp.send('Profiler.stop'); writeFileSync(join(tmpdir(), 'coflow-selection.cpuprofile'), JSON.stringify(profile)); console.log(profile.nodes.filter(n => n.hitCount > 0).sort((a,b)=>b.hitCount-a.hitCount).slice(0,25).map(n=>({ name:n.callFrame.functionName, line:n.callFrame.lineNumber, hits:n.hitCount, url:n.callFrame.url }))) }
    frames.sort((a,b)=>a-b)
    const result = { rows, cols, inspector, readyMs, mountedRows: await page.locator('tr[data-coordinate-id]').count(), textareas: await page.locator('.cell-string-textarea').count(), frameP50: Math.round(frames[Math.floor(frames.length*.5)]), frameP95: Math.round(frames[Math.floor(frames.length*.95)]), framesOver33: frames.filter(t=>t>33.4).length, scrollMs: Object.fromEntries(['ScriptDuration','LayoutDuration','TaskDuration'].map(key=>[key,Math.round((after[key]-before[key])*1000)])), selectAllMs, errors }
    results.push(result); console.log(JSON.stringify(result)); await page.close()
  }
} finally { await browser.close(); await new Promise(r => server.httpServer.close(r)); writeFileSync(join(tmpdir(), `coflow-performance-${baseline ? 'baseline' : 'current'}.json`), JSON.stringify(results,null,2)) }
