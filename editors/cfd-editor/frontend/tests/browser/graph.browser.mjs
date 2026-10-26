import assert from 'node:assert/strict'
import { after, before, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, strictPort: false, host: '127.0.0.1' } })
  await server.listen()
  browser = await chromium.launch({ headless: true })
  page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
  page.setDefaultTimeout(7000)
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

test('sparse wire annotations load scalar, reference, object and dictionary graph rows', async () => {
  for (const query of ['kind=refs', 'kind=scalar', 'kind=objects', 'kind=dict']) {
    await openFixture(`items=8&editable&wire&${query}`)
    const source = page.locator('.graph-node[data-nodeid="Item::source"]')
    if (!query.includes('refs')) {
      await source.locator('.dc-row[data-field-path="values"]').click()
      await page.waitForFunction(() => window.graphFlow.getNode('Item::source').data.geometry.pendingCount === 0)
    }
    assert.equal(await source.count(), 1)
    assert.equal(await page.evaluate(() => window.graphHarness.getData().edges.length), 8)
    assert.ok(await page.locator('.react-flow__edge').count() > 0)
    assert.ok(await source.locator('.dc-row').count() > 0)
    assert.deepEqual(errors, [])
  }
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
    const boxes = await source.evaluate(root => {
      const node = window.graphFlow.getInternalNode('Item::source')
      const zoom = window.graphFlow.getViewport().zoom, origin = root.getBoundingClientRect()
      return [...root.querySelectorAll('.react-flow__handle')].map(element => {
        const rect = element.getBoundingClientRect()
        const box = [...node.internals.handleBounds.source, ...node.internals.handleBounds.target].find(handle => handle.id === element.dataset.handleid)
        return Math.max(Math.abs((rect.x - origin.x) / zoom - box.x), Math.abs((rect.y - origin.y) / zoom - box.y), Math.abs(rect.width / zoom - box.width), Math.abs(rect.height / zoom - box.height))
      })
    })
    assert.ok(boxes.every(value => value <= 1), JSON.stringify(boxes))
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

test('public node geometry keeps an unmounted source anchor reconnectable from the visible edge body', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/graph-geometry.html`)
  const edge = page.locator('.graph-edge-drag')
  await edge.waitFor()
  assert.equal(await page.locator('.react-flow__handle.source').count(), 0)
  const anchor = await page.evaluate(() => {
    const node = window.geometryFlow.getInternalNode('source')
    return node.internals.handleBounds.source.find(handle => handle.id === 'row-60')
  })
  assert.equal(anchor.y, 2055)
  // 源锚点在屏幕下方；拖动屏内线体仍能启动目标端重连。
  const point = await edge.evaluate(element => {
    const point = element.getPointAtLength(element.getTotalLength() * 0.9)
    const screen = new DOMPoint(point.x, point.y).matrixTransform(element.getScreenCTM())
    return { x: screen.x, y: screen.y }
  })
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await page.mouse.move(850, 421, { steps: 12 })
  await page.mouse.up()
  await page.waitForFunction(() => window.geometryHarness.events.length > 0)
  assert.deepEqual(await page.evaluate(() => window.geometryHarness.events[0]), {
    source: 'source', sourceHandle: 'row-60', target: 'other', targetHandle: 'in',
  })
  assert.deepEqual(errors, [])
})

async function openFixture(query) {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/graph-performance.html?${query}`)
  await page.waitForFunction(() => window.graphFlow?.getNodes().length > 0)
  await settle()
  await page.evaluate(() => window.graphFlow.setViewport({ x: 100, y: 100, zoom: 1 }))
  await page.waitForFunction(() => window.graphFlow.getNodes().every(node => node.data.geometry.pendingCount === 0))
  await settle()
}

test('ten thousand duplicate references retain full geometry with bounded rows and handles during pan and compact mode', async () => {
  const counts = []
  for (const count of [1000, 10000]) {
    await openFixture(`items=${count}&standard&editable`)
    await page.evaluate(() => {
      const node = window.graphFlow.getNode('Item::source')
      window.beforeGeometry = { handles: node.handles, model: node.data.geometry, height: node.height }
    })
    await page.evaluate(count => window.graphFlow.setViewport({ x: 100, y: -count * 15, zoom: 1 }), count)
    await settle()
    const metrics = await page.evaluate(() => {
      const node = window.graphFlow.getNode('Item::source')
      const root = document.querySelector('.graph-node[data-nodeid="Item::source"]')
      return { rows: root.querySelectorAll('.dc-row').length, handles: root.querySelectorAll('.react-flow__handle').length,
        logical: window.graphFlow.getInternalNode(node.id).internals.handleBounds.source.length,
        unchanged: node.handles === window.beforeGeometry.handles && node.data.geometry === window.beforeGeometry.model && node.height === window.beforeGeometry.height }
    })
    assert.equal(metrics.logical, count + 1)
    assert.ok(metrics.rows <= 200 && metrics.handles <= 200, JSON.stringify(metrics))
    assert.equal(metrics.unchanged, true)
    counts.push(metrics.rows)
    await page.evaluate(() => window.graphHarness.setCompact(true))
    await settle()
    assert.equal(await page.locator('.graph-node[data-nodeid="Item::source"] .dc-row').count(), 0)
    assert.equal(await page.evaluate(() => window.graphFlow.getNode('Item::source').height === window.beforeGeometry.height), true)
  }
  assert.ok(counts[1] <= counts[0] * 1.1, JSON.stringify(counts))
  assert.deepEqual(errors, [])
})

test('a visible crossing edge with both nodes and its source row unmounted retains body dragging and array deletion', async () => {
  await openFixture('items=1000&standard&editable')
  await page.evaluate(() => window.graphFlow.setViewport({ x: -50000, y: -26000, zoom: 1 }))
  await settle()
  assert.equal(await page.locator('.react-flow__node').count(), 0)
  const point = await page.evaluate(() => {
    const edge = window.graphFlow.getEdges().find(edge => edge.sourceHandle === 'path-links[600]')
    const path = document.querySelector(`.react-flow__edge[data-id="${CSS.escape(edge.id)}"] .graph-edge-drag`)
    for (let length = 0; length < path.getTotalLength(); length += 50) {
      const point = path.getPointAtLength(length)
      const screen = new DOMPoint(point.x, point.y).matrixTransform(path.getScreenCTM())
      if (screen.x > 100 && screen.x < 1200 && screen.y > 100 && screen.y < 700) return { x: screen.x, y: screen.y }
    }
    throw new Error('crossing edge not visible')
  })
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await page.mouse.move(point.x + 90, point.y + 80, { steps: 12 })
  await page.mouse.up()
  await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'collection'))
  const event = await page.evaluate(() => window.graphHarness.events.find(event => event.kind === 'collection'))
  assert.deepEqual(event.path, [{ kind: 'field', value: 'links' }])
  assert.deepEqual(event.operation, { kind: 'array_remove', index: 600 })
  assert.deepEqual(errors, [])
})

test('the last append port remains usable after its preceding rows have been unmounted', async () => {
  await openFixture('items=1000&standard&editable')
  await page.evaluate(() => {
    const node = window.graphFlow.getNode('Item::source')
    window.graphFlow.setViewport({ x: 100, y: -node.data.geometry.center('links[+]') + 300, zoom: 1 })
  })
  await settle()
  const handle = page.locator('[data-handleid="path-links[+]"]')
  const rect = await handle.boundingBox()
  await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2)
  await page.mouse.down()
  await page.mouse.move(800, 450, { steps: 12 })
  await page.mouse.up()
  const chooser = page.getByRole('combobox', { name: '选择引用目标' })
  await chooser.waitFor()
  await chooser.fill('other')
  await page.getByRole('option').filter({ hasText: 'other' }).click()
  await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'collection'))
  const event = await page.evaluate(() => window.graphHarness.events.find(event => event.kind === 'collection'))
  assert.deepEqual(event.path, [{ kind: 'field', value: 'links' }])
  assert.deepEqual(event.operation, { kind: 'array_append', value: { kind: 'ref', value: 'other' } })
  assert.deepEqual(errors, [])
})

test('a composing text editor and its draft remain mounted while the viewport moves away and back', async () => {
  await openFixture('items=1000&kind=strings&editable')
  const source = page.locator('.graph-node[data-nodeid="Item::source"]')
  await source.locator('.dc-row[data-field-path="values"]').click()
  const editor = source.locator('.dc-row[data-field-path="values[0]"] textarea')
  await editor.waitFor()
  await editor.focus()
  await editor.dispatchEvent('compositionstart', { data: '' })
  await editor.fill('中文草稿\n第二行')
  const element = await editor.elementHandle()
  await page.evaluate(() => window.graphFlow.setViewport({ x: -50000, y: -30000, zoom: 1 }))
  await settle()
  assert.equal(await element.evaluate(element => element.isConnected && element === document.activeElement), true)
  assert.equal(await editor.inputValue(), '中文草稿\n第二行')
  assert.equal(await page.evaluate(() => window.graphHarness.events.length), 0)
  await page.evaluate(() => window.graphFlow.setViewport({ x: 100, y: 100, zoom: 1 }))
  await settle()
  assert.equal(await editor.evaluate((editor, original) => editor === original, element), true)
  await editor.dispatchEvent('compositionend', { data: '中文草稿' })
  await editor.blur()
  await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'write'))
  const event = await page.evaluate(() => window.graphHarness.events.find(event => event.kind === 'write'))
  assert.deepEqual(event.path, [{ kind: 'field', value: 'values' }, { kind: 'index', value: 0 }])
  assert.equal(event.value.value, '中文草稿\n第二行')
  assert.deepEqual(errors, [])
})

test('complex Option objects and special dictionary keys keep expansion and structured writes across windows', async () => {
  for (const kind of ['objects', 'dict']) {
    await openFixture(`items=60&kind=${kind}&editable&reference`)
    const source = page.locator('.graph-node[data-nodeid="Item::source"]')
    await source.locator('.dc-row[data-field-path="values"]').click()
    if (kind === 'objects') await source.locator('.dc-row[data-field-path="values[0]"]').click()
    const editor = source.locator('.dc-row textarea').first()
    // name 是第一个字符串控件；按结构化路径选择目标，避免展示键的转义歧义。
    const target = kind === 'objects' ? source.locator('.dc-row[data-field-path="values[0].title"] textarea')
      : source.locator('.dc-row[data-field-path-wire]').filter({ has: page.locator('textarea') }).nth(1).locator('textarea')
    await target.fill('窗口内修改')
    await target.blur()
    await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'write'))
    const event = await page.evaluate(() => window.graphHarness.events.find(event => event.kind === 'write'))
    assert.deepEqual(event.path, kind === 'objects'
      ? [{ kind: 'field', value: 'values' }, { kind: 'index', value: 0 }, { kind: 'field', value: 'title' }]
      : [{ kind: 'field', value: 'values' }, { kind: 'dict_key', value: JSON.stringify('key0.x["z"]') }])
    assert.equal(event.value.value, '窗口内修改')
    await page.evaluate(() => window.graphFlow.setViewport({ x: 100, y: -1700, zoom: 1 }))
    await settle()
    await page.evaluate(() => window.graphFlow.setViewport({ x: 100, y: 100, zoom: 1 }))
    await settle()
    assert.equal(await target.inputValue(), '窗口内修改')
    assert.ok(await editor.count())
    assert.deepEqual(errors, [])
  }
})

test('dynamic and complex heights match an ordinary complete card before and after typography invalidation', async () => {
  for (const query of ['kind=strings', 'kind=objects', 'kind=dict', 'kind=objects&editable']) {
    await openFixture(`items=60&${query}&reference`)
    const source = page.locator('.graph-node[data-nodeid="Item::source"]')
    await source.locator('.dc-row[data-field-path="values"]').click()
    const compare = async () => {
      await page.waitForFunction(() => window.graphFlow.getNode('Item::source').data.geometry.pendingCount === 0 && document.querySelector('.full-reference').getAnimations({ subtree: true }).every(animation => animation.playState === 'finished'))
      await settle()
      const result = await page.evaluate(() => {
        const geometry = window.graphFlow.getNode('Item::source').data.geometry
        const card = document.querySelector('.full-reference .dc-node-card')
        const origin = card.getBoundingClientRect().top
        const differences = [...card.querySelectorAll('.dc-row[data-field-path-wire]')].map(row => {
          const rect = row.getBoundingClientRect()
          return { path: row.dataset.fieldPath, diff: Math.abs(geometry.center(row.dataset.fieldPath) - geometry.headerHeight - geometry.borderTop - (rect.top - origin + rect.height / 2)) }
        })
        return { height: Math.abs(card.getBoundingClientRect().height - geometry.bodyHeight), differences }
      })
      assert.ok(result.height <= 1 && result.differences.every(row => row.diff <= 1), JSON.stringify(result))
    }
    await compare()
    await page.evaluate(() => { document.body.style.fontSize = '16px'; document.fonts.dispatchEvent(new Event('loadingdone')) })
    await compare()
    if (!query.includes('editable')) assert.equal(await source.locator('textarea, input').count(), 0)
    assert.deepEqual(errors, [])
  }
})

test('foreign array deletion ends an old draft and rejected writes remain editable', async () => {
  await openFixture('items=60&kind=strings&editable')
  const source = page.locator('.graph-node[data-nodeid="Item::source"]')
  await source.locator('.dc-row[data-field-path="values"]').click()
  const editor = source.locator('.dc-row[data-field-path="values[0]"] textarea')
  await editor.fill('不能写到新元素')
  await page.evaluate(() => window.graphHarness.setData(previous => ({ ...previous, revision: previous.revision + 1, nodes: previous.nodes.map(node => node.coordinate.id !== 'source' ? node : {
    ...node, fields: node.fields.map(field => field.name !== 'values' ? field : { ...field, value: { ...field.value, value: field.value.value.slice(1) } }),
  }) })))
  await page.waitForFunction(() => document.querySelector('.graph-node[data-nodeid="Item::source"] .dc-row[data-field-path="values[0]"] textarea')?.value === '文字 1\n第二行文字 1')
  assert.equal(await editor.inputValue(), '文字 1\n第二行文字 1')
  assert.equal(await page.evaluate(() => window.graphHarness.events.length), 0)
  await page.evaluate(() => window.graphHarness.setFailWrites(true))
  await editor.fill('失败后重试')
  await editor.blur()
  await page.getByText('fixture write rejected', { exact: false }).first().waitFor()
  assert.equal(await editor.inputValue(), '失败后重试')
  await page.evaluate(() => window.graphHarness.setFailWrites(false))
  await editor.focus()
  await editor.fill('重新提交')
  await editor.blur()
  await page.waitForFunction(() => window.graphHarness.events.length === 2)
  assert.equal(await editor.inputValue(), '重新提交')
  assert.deepEqual(errors, [])
})

test('an offscreen append source supports canceling the picker and creating a new connected record', async () => {
  await openFixture('items=1000&standard&editable&create')
  const openPicker = async () => {
    await page.evaluate(() => {
      const node = window.graphFlow.getNode('Item::source')
      window.graphFlow.setViewport({ x: 100, y: -node.data.geometry.center('links[+]') + 300, zoom: 1 })
    })
    await settle()
    const box = await page.locator('[data-handleid="path-links[+]"]').boundingBox()
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(800, 450, { steps: 12 })
    await page.mouse.up()
    await page.getByRole('combobox', { name: '选择引用目标' }).waitFor()
  }
  await openPicker()
  await page.keyboard.press('Escape')
  await page.getByRole('combobox', { name: '选择引用目标' }).waitFor({ state: 'hidden' })
  assert.equal(await page.getByRole('combobox', { name: '选择引用目标' }).count(), 0)
  assert.equal(await page.evaluate(() => window.graphHarness.events.length), 0)
  await openPicker()
  await page.getByRole('option').filter({ hasText: '新建节点' }).click()
  const dialog = page.getByRole('dialog', { name: '新建记录', exact: true })
  await dialog.getByRole('textbox', { name: '记录 ID' }).fill('created')
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await page.waitForFunction(() => window.graphHarness.events.some(event => event.kind === 'collection'))
  const events = await page.evaluate(() => window.graphHarness.events)
  assert.deepEqual(events.find(event => event.kind === 'insert'), { kind: 'insert', id: 'created', actual_type: 'Item' })
  assert.deepEqual(events.find(event => event.kind === 'collection').operation, { kind: 'array_append', value: { kind: 'ref', value: 'created' } })
  assert.deepEqual(errors, [])
})
