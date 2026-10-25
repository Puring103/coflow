import { createServer } from 'vite'
import { chromium } from 'playwright'
import { fileURLToPath } from 'node:url'

// 真实表格的浏览器基准：固定视窗，只改变数据量，统计全选和挂载节点数。
const server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, host: '127.0.0.1' } })
let browser
try {
  await server.listen()
  browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  for (const rows of [100, 1000, 3000]) {
    await page.goto(`${server.resolvedUrls.local[0]}tests/browser/table.html?rows=${rows}`)
    await page.locator('.cell-string-textarea').first().waitFor()
    await page.locator('.cell-string-textarea').first().click()
    const samples = []
    for (let sample = 0; sample < 5; sample++) {
      await page.keyboard.press('ArrowRight')
      const start = performance.now()
      await page.keyboard.press('Control+a')
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      samples.push(Math.round(performance.now() - start))
    }
    console.log(JSON.stringify({ rows, mountedRows: await page.locator('tr[data-coordinate-id]').count(), textareas: await page.locator('.cell-string-textarea').count(), selectAllMs: samples, medianMs: samples.sort((a,b) => a-b)[2] }))
  }
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/table.html?dimensionRows=3000`)
  await page.locator('[data-dimension-row="0"]').first().waitFor()
  console.log(JSON.stringify({ dimensionRows: 3000, mountedRows: await page.locator('.dimension-table tr[data-index]').count(), textareas: await page.locator('.cell-string-textarea').count() }))
} finally { await browser?.close(); await server.close() }
