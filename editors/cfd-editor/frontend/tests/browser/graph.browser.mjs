import assert from 'node:assert/strict'
import { after, before, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, host: '127.0.0.1' } })
  await server.listen()
  browser = await chromium.launch({ headless: true })
  page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
  page.on('pageerror', error => errors.push(error.message))
})
after(async () => { await browser?.close(); await server?.close() })
beforeEach(async () => {
  errors.length = 0
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/graph-performance.html?items=8&editable`)
  await page.locator('.graph-node').first().waitFor()
  await page.waitForFunction(() => window.graphFlow?.getNodes().length > 0)
  await page.evaluate(() => window.graphFlow.setViewport({ x: 100, y: 100, zoom: 1 }))
  await settle()
})

test('duplicate targets retain distinct source handles and accurate row anchors through zoom and compact mode', async () => {
  const source = page.locator('.graph-node[data-nodeid="Item::source"]')
  const initial = await page.evaluate(() => {
    const flow = window.graphFlow
    const source = flow.getNodes().find(node => node.data.graphNode.record_id === 'source')
    return { id: source.id, height: flow.getInternalNode(source.id).measured.height }
  })
  for (const zoom of [0.5, 1, 1.5]) {
    await page.evaluate(zoom => window.graphFlow.setViewport({ x: 100, y: 100, zoom }), zoom)
    await settle()
    const differences = await source.evaluate(root => Array.from({ length: 8 }, (_, index) => {
      const row = root.querySelector(`.dc-row[data-field-path="links[${index}]"]`)
      const handle = root.querySelector(`[data-handleid="path-links[${index}]"]`)
      const a = row.getBoundingClientRect(), b = handle.getBoundingClientRect()
      return Math.abs((a.top + a.bottom) / 2 - (b.top + b.bottom) / 2)
    }))
    assert(differences.every(value => value <= 1), JSON.stringify(differences))
  }
  await page.evaluate(() => window.graphHarness.setCompact(true))
  await settle()
  assert.equal(await page.evaluate(id => window.graphFlow.getInternalNode(id).measured.height, initial.id), initial.height)
  await page.evaluate(() => window.graphHarness.setCompact(false))
  await settle()
  assert.equal(await page.locator('.react-flow__edge').count(), 8)
  assert.deepEqual(errors, [])
})

test('reference port drag retains the structured list path', async () => {
  const handle = page.locator('[data-handleid="path-links[0]"]')
  const target = page.locator('.graph-node[data-nodeid="Item::target"]')
  const a = await handle.boundingBox(), b = await target.boundingBox()
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2)
  await page.mouse.down()
  await page.mouse.move(b.x + 5, b.y + 20, { steps: 10 })
  await page.mouse.up()
  await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'write'))
  const event = await page.evaluate(() => window.graphHarness.events.find(event => event.kind === 'write'))
  assert.deepEqual(event.path, [{ kind: 'field', value: 'links' }, { kind: 'index', value: 0 }])
  assert.equal(event.value.kind, 'ref')
  assert.equal(event.value.value, 'target')
  assert.deepEqual(errors, [])
})
