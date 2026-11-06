import assert from 'node:assert/strict'
import { after, before, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, host: '127.0.0.1', strictPort: false } })
  await server.listen()
  browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
  page = await browser.newPage({ viewport: { width: 1100, height: 850 } })
  page.setDefaultTimeout(5000)
  page.on('pageerror', error => errors.push(error.message))
})
after(async () => { await browser?.close(); await server?.close() })
beforeEach(async () => {
  errors.length = 0
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html`)
  await page.waitForFunction(() => window.motionTest && document.querySelector('td[data-field="name"]'))
})
const open = async () => {
  await page.evaluate(() => window.motionTest.open())
  await page.locator('.inspector-modal').waitFor()
}
const close = () => page.evaluate(() => document.querySelector('[aria-label="关闭编辑弹窗"]').click())

test('expansion starts at the visible source and never scales or reflows text', async () => {
  await open()
  const geometry = await page.evaluate(() => {
    const shell = document.querySelector('.inspector-motion-shell')
    const panel = document.querySelector('.inspector-modal')
    const animation = shell.getAnimations()[0]
    animation.pause()
    animation.currentTime = 0
    const start = shell.getBoundingClientRect().toJSON()
    const width = panel.getBoundingClientRect().width
    animation.currentTime = 110
    return { start, source: window.motionTest.bounds().toJSON(), width,
      middleWidth: panel.getBoundingClientRect().width, transform: getComputedStyle(panel).transform,
      shellWidth: shell.getBoundingClientRect().width }
  })
  assert.ok(Math.abs(geometry.start.x - geometry.source.x) < 2)
  assert.ok(Math.abs(geometry.start.y - geometry.source.y) < 2)
  assert.ok(Math.abs(geometry.start.width - geometry.source.width) < 2)
  assert.equal(geometry.width, geometry.middleWidth)
  assert.equal(geometry.transform, 'none')
  assert.ok(geometry.shellWidth > geometry.start.width)
  assert.deepEqual(errors, [])
})

test('closing during entry continues from the current frame and restores focus', async () => {
  await open()
  await page.evaluate(() => {
    const animation = document.querySelector('.inspector-motion-shell').getAnimations()[0]
    animation.pause(); animation.currentTime = 65
  })
  const before = await page.locator('.inspector-motion-shell').boundingBox()
  await close()
  await page.waitForFunction(() => document.querySelector('.inspector-motion-shell')?.getAnimations()[0]?.effect.getTiming().duration === 120)
  const from = await page.evaluate(() => {
    const shell = document.querySelector('.inspector-motion-shell')
    const animation = shell.getAnimations()[0]
    animation.pause(); animation.currentTime = 0
    const rect = shell.getBoundingClientRect().toJSON()
    animation.finish()
    return rect
  })
  assert.ok(Math.abs(before.x - from.x) < 2)
  assert.ok(Math.abs(before.width - from.width) < 2)
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => document.activeElement.classList.contains('table-scroll')), true)
  assert.equal(await page.locator('.app').evaluate(el => el.inert), false)
  assert.deepEqual(errors, [])
})

test('slow or failed saving keeps the editor and blocks duplicate close requests', async () => {
  await open()
  await page.evaluate(() => { window.motionTest.control.fail = true; window.motionTest.control.delay = 350 })
  await close()
  await close()
  await page.getByRole('status').filter({ hasText: '正在保存' }).waitFor()
  assert.equal(await page.locator('.inspector-modal').evaluate(el => el.inert), true)
  await page.getByRole('status').filter({ hasText: '保存失败' }).waitFor()
  assert.equal(await page.locator('.inspector-modal').evaluate(el => el.inert), false)
  assert.equal(await page.locator('.inspector-modal').evaluate(el => el.style.width), '')
  assert.equal(await page.evaluate(() => window.motionTest.control.commits), 1)
  assert.equal(await page.evaluate(() => window.motionTest.control.closes), 0)
  await page.evaluate(() => { window.motionTest.control.fail = false; window.motionTest.control.delay = 0 })
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('missing source fades out and reduced motion skips geometry animations', async () => {
  await open()
  await page.waitForTimeout(250)
  await page.evaluate(() => window.motionTest.remove())
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  await page.reload()
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.waitForFunction(() => window.motionTest)
  await open()
  assert.equal(await page.locator('.inspector-motion-shell').evaluate(el => el.getAnimations().length), 0)
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('navigation emphasizes after arrival and user input interrupts short scrolling', async () => {
  await page.evaluate(() => window.motionTest.reveal(5))
  assert.equal(await page.locator('#target-5').evaluate(el => el.classList.contains('motion-target')), false)
  await page.waitForFunction(() => document.getElementById('target-5').classList.contains('motion-target'))
  const arrived = await page.evaluate(() => {
    const target = document.getElementById('target-5').getBoundingClientRect()
    const root = document.getElementById('scroller').getBoundingClientRect()
    return target.top >= root.top && target.bottom <= root.bottom
  })
  assert.equal(arrived, true)
  await page.evaluate(() => {
    window.motionTest.reveal(8)
    document.getElementById('scroller').dispatchEvent(new WheelEvent('wheel'))
  })
  const top = await page.locator('#scroller').evaluate(el => el.scrollTop)
  await page.waitForTimeout(220)
  assert.equal(await page.locator('#scroller').evaluate(el => el.scrollTop), top)
  assert.equal(await page.locator('#target-8').evaluate(el => el.classList.contains('motion-target')), false)
  assert.deepEqual(errors, [])
})

test('return geometry uses the current source location and resize releases fixed content dimensions', async () => {
  await open()
  await page.waitForTimeout(250)
  await page.setViewportSize({ width: 900, height: 700 })
  assert.ok((await page.locator('.inspector-modal').boundingBox()).width <= 852)
  await page.evaluate(() => {
    document.querySelector('.table-view').style.transform = 'translateX(30px)'
    const shell = document.querySelector('.inspector-motion-shell')
    const animate = shell.animate.bind(shell)
    // 在创建退场动画的同一帧采集最终几何；resize 落定可能立即结束动画并卸载窗口。
    shell.animate = (frames, options) => {
      const animation = animate(frames, options)
      animation.pause()
      animation.currentTime = animation.effect.getTiming().duration
      window.returnGeometry = {
        end: shell.getBoundingClientRect().toJSON(),
        source: window.motionTest.bounds().toJSON(),
      }
      animation.finish()
      return animation
    }
  })
  await close()
  await page.waitForFunction(() => window.returnGeometry)
  const geometry = await page.evaluate(() => window.returnGeometry)
  assert.ok(Math.abs(geometry.end.x - geometry.source.x) < 2)
  assert.ok(Math.abs(geometry.end.y - geometry.source.y) < 2)
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  await page.setViewportSize({ width: 1100, height: 850 })
  assert.deepEqual(errors, [])
})

test('only committed history changes emphasize targets, including successful undo', async () => {
  await page.evaluate(() => window.motionTest.commit())
  const cell = page.locator('td[data-field="name"]').first()
  await page.waitForFunction(() => document.querySelector('td[data-field="name"]').classList.contains('motion-target'))
  await page.waitForTimeout(1050)
  await page.evaluate(() => window.motionTest.failedUndo())
  assert.equal(await cell.evaluate(el => el.classList.contains('motion-target')), false)
  await page.evaluate(() => window.motionTest.undo())
  await page.waitForFunction(() => document.querySelector('td[data-field="name"]').classList.contains('motion-target'))
  assert.deepEqual(errors, [])
})

test('new records are selected after arrival and deletion moves focus to a remaining row', async () => {
  await page.locator('td.sticky-key-column').first().click({ button: 'right' })
  await page.getByRole('menuitem', { name: '在下方插入记录' }).click()
  await page.getByRole('textbox', { name: '记录 ID', exact: true }).fill('new_motion_row')
  await page.getByRole('button', { name: '创建', exact: true }).click()
  const row = page.locator('tr[data-coordinate-id]').filter({ hasText: 'new_motion_row' })
  await row.locator('xpath=self::tr[contains(@class, "selected")]').waitFor()
  assert.equal(await row.evaluate(el => el.classList.contains('selected')), true)
  await page.waitForFunction(() => document.activeElement.classList.contains('table-scroll'))
  await row.locator('td.sticky-key-column').click({ button: 'right' })
  await page.locator('.record-context-menu').getByRole('menuitem', { name: /删除/ }).click()
  await page.getByRole('dialog', { name: '删除记录' }).getByRole('button', { name: '删除', exact: true }).click()
  await row.waitFor({ state: 'detached' })
  await page.waitForFunction(() => document.activeElement.classList.contains('table-scroll'))
  assert.equal(await page.locator('tr.selected').count(), 1)
  assert.deepEqual(errors, [])
})

test('settled and reduced-motion windows keep their content inside the frame at small sizes', async () => {
  for (const reducedMotion of ['no-preference', 'reduce']) {
    await page.emulateMedia({ reducedMotion })
    await page.setViewportSize({ width: 460, height: 360 })
    await open()
    await page.waitForTimeout(240)
    const geometry = await page.evaluate(() => ({
      shell: document.querySelector('.inspector-motion-shell').getBoundingClientRect().toJSON(),
      panel: document.querySelector('.inspector-modal').getBoundingClientRect().toJSON(),
    }))
    assert.ok(geometry.panel.right <= geometry.shell.right)
    assert.ok(geometry.panel.bottom <= geometry.shell.bottom)
    assert.ok(geometry.shell.right <= 460 - 24)
    assert.ok(geometry.shell.bottom <= 360 - 24)
    await close()
    await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  }
  await page.setViewportSize({ width: 1100, height: 850 })
  assert.deepEqual(errors, [])
})

test('ordinary dialog cancellation fades out once and unmounts after the exit', async () => {
  await page.locator('td.sticky-key-column').first().click({ button: 'right' })
  await page.getByRole('menuitem', { name: '在下方插入记录' }).click()
  const dialog = page.getByRole('dialog', { name: '新建记录' })
  await dialog.waitFor()
  await page.evaluate(() => document.querySelector('.create-record-close').click())
  assert.equal(await dialog.evaluate(element => element.inert), true)
  await dialog.waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('an edited modal emphasizes its source after return without animating the isolated background', async () => {
  await open()
  await page.evaluate(() => window.motionTest.commit())
  await page.waitForTimeout(40)
  const source = page.locator('td[data-field="name"]').first()
  assert.equal(await source.evaluate(element => element.classList.contains('motion-target')), false)
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.equal(await source.evaluate(element => element.classList.contains('motion-target')), true)
  assert.deepEqual(errors, [])
})

test('the workspace opens cells from their source and returns keyboard focus', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const cell = page.locator('td.sticky-key-column').first()
  await cell.waitFor()
  // 直接双击单元格空白表面，避免文本输入控件接管原位编辑。
  await cell.dispatchEvent('dblclick')
  await page.locator('.inspector-modal').waitFor()
  assert.equal(await page.locator('.app').evaluate(element => element.inert), true)
  await page.keyboard.press('Escape')
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => document.activeElement.classList.contains('table-scroll')), true)
  assert.deepEqual(errors, [])
})

test('complex cells expand from the whole cell instead of a nested text preview', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?complex=1`)
  const cell = page.locator('td.complex-cell').first()
  await cell.hover()
  await cell.locator('.cell-expand-button').click()
  await page.locator('.inspector-modal').waitFor()
  const bounds = await cell.boundingBox()
  const source = await page.evaluate(() => window.motionTest.bounds().toJSON())
  assert.ok(Math.abs(bounds.width - source.width) < 2)
  assert.ok(Math.abs(bounds.height - source.height) < 2)
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('backdrop click preserves the selection until the source return animation finishes', async () => {
  await open()
  await page.waitForTimeout(200)
  await page.mouse.click(8, 8)
  await page.waitForFunction(() => document.querySelector('.inspector-motion-shell')?.getAnimations().length > 0)
  const result = await page.evaluate(() => {
    const shell = document.querySelector('.inspector-motion-shell')
    const animation = shell?.getAnimations()[0]
    if (!animation) return null
    animation.pause()
    const frames = animation.effect.getKeyframes()
    const result = { duration: animation.effect.getTiming().duration, destination: frames[frames.length - 1].transform,
      closes: window.motionTest.control.closes, selected: !!document.querySelector('td.selected-cell') }
    animation.finish()
    return result
  })
  assert.ok(result)
  assert.equal(result.duration, 120)
  assert.match(result.destination, /translate\(/)
  assert.equal(result.closes, 0)
  assert.equal(result.selected, true)
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.motionTest.control.closes), 1)
  assert.deepEqual(errors, [])
})

test('backdrop close retains the modal when saving fails', async () => {
  await open()
  await page.evaluate(() => { window.motionTest.control.fail = true })
  await page.mouse.click(8, 8)
  await page.getByRole('status').filter({ hasText: '保存失败' }).waitFor()
  assert.equal(await page.locator('.inspector-modal').count(), 1)
  assert.equal(await page.locator('td.selected-cell').count(), 1)
  assert.equal(await page.evaluate(() => window.motionTest.control.closes), 0)
  assert.deepEqual(errors, [])
})

test('reference entry, nested navigation, backdrop return and close animate without clearing the editor below', async () => {
  await open()
  await page.waitForTimeout(200)
  await page.evaluate(() => window.motionTest.reference())
  const panel = page.locator('.reference-panel')
  await panel.waitFor()
  const entry = await panel.evaluate(element => {
    const animation = element.getAnimations()[0]
    const frames = animation.effect.getKeyframes()
    animation.finish()
    return frames[0].transform
  })
  assert.equal(entry, 'translateY(6px)')
  await page.getByRole('button', { name: '打开下一引用' }).click()
  await page.getByRole('dialog', { name: '引用记录：B', exact: true }).waitFor()
  await page.mouse.click(8, 8)
  await page.getByRole('dialog', { name: '引用记录：A', exact: true }).waitFor()
  assert.equal(await panel.evaluate(element => element.inert), false)
  assert.equal(await page.locator('.inspector-modal').count(), 1)
  await page.waitForTimeout(120)
  await page.mouse.click(8, 8)
  await page.waitForFunction(() => document.querySelector('.reference-panel')?.getAnimations().length > 0)
  const exit = await panel.evaluate(element => {
    const animation = element.getAnimations()[0]
    animation.pause()
    const duration = animation.effect.getTiming().duration
    animation.finish()
    return duration
  })
  assert.equal(exit, 120)
  await panel.waitFor({ state: 'detached' })
  assert.equal(await page.locator('.inspector-modal').count(), 1)
  assert.equal(await page.evaluate(() => window.motionTest.control.closes), 0)
  assert.deepEqual(errors, [])
})

test('reference navigation honors reduced motion', async () => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.evaluate(() => window.motionTest.reference())
  const panel = page.locator('.reference-panel')
  await panel.waitFor()
  assert.equal(await panel.evaluate(element => element.getAnimations().length), 0)
  await page.getByRole('button', { name: '打开下一引用' }).click()
  await page.getByRole('button', { name: '返回', exact: true }).click()
  await page.getByRole('dialog', { name: '引用记录：A', exact: true }).waitFor()
  await page.getByRole('button', { name: '关闭引用记录' }).click()
  await panel.waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('foldout keeps header geometry and reverses collapse continuously', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?complex=1`)
  await page.waitForFunction(() => window.motionTest)
  await page.evaluate(() => window.motionTest.record())
  const header = page.locator('.inspector-modal .dc-row-foldout').first()
  await header.waitFor()
  assert.ok(await header.locator('.vc-ref').count() > 0)
  const before = await header.evaluate(e => e.getBoundingClientRect().height)
  await header.evaluate(e => e.click())
  const body = header.locator('..').locator(':scope > .dc-group-body')
  await body.waitFor()
  assert.equal(await header.evaluate(e => e.getBoundingClientRect().height), before)
  await body.evaluate(e => e.getAnimations().forEach(a => a.finish()))
  await header.evaluate(e => e.click())
  const middle = await body.evaluate(e => {
    const a = e.getAnimations()[0]; a.pause(); a.currentTime = 70
    return { height: e.getBoundingClientRect().height, inert: e.inert }
  })
  assert.equal(middle.inert, true)
  assert.ok(middle.height > 0)
  await header.evaluate(e => e.click())
  const reversed = await body.evaluate(e => {
    const a = e.getAnimations()[0]; a.pause(); a.currentTime = 0
    return { height: e.getBoundingClientRect().height, inert: e.inert }
  })
  assert.ok(Math.abs(middle.height - reversed.height) < 1)
  assert.equal(reversed.inert, false)
  await body.evaluate(e => e.getAnimations().forEach(a => a.finish()))
  await header.evaluate(e => e.click())
  await body.waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})

test('workspace blank clears selection and view tabs animate without moving content', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const cell = page.locator('td[data-field="name"]').first()
  await cell.click()
  assert.ok(await page.locator('td.selected-cell').count() > 0)
  await page.locator('.table-scroll').dispatchEvent('pointerdown', { button: 0 })
  assert.equal(await page.locator('td.selected-cell').count(), 0)
  const tabs = page.locator('.view-picker [role="tab"]')
  assert.ok(await tabs.count() > 1)
  await tabs.nth(1).evaluate(e => e.click())
  const state = await tabs.nth(1).evaluate(e => ({ selected: e.getAttribute('aria-selected'), animation: getComputedStyle(e).animationName }))
  assert.equal(state.selected, 'true')
  assert.equal(state.animation, 'none')
  assert.deepEqual(errors, [])
})

test('foldout reduced motion removes children immediately', async () => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?complex=1`)
  await page.waitForFunction(() => window.motionTest)
  await page.evaluate(() => window.motionTest.record())
  const header = page.locator('.inspector-modal .dc-row-foldout').first()
  await header.evaluate(e => e.click())
  const body = header.locator('..').locator(':scope > .dc-group-body')
  assert.equal(await body.evaluate(e => e.getAnimations().length), 0)
  await header.evaluate(e => e.click())
  assert.equal(await body.count(), 0)
  assert.deepEqual(errors, [])
})

test('sidebar selection moves its background without moving the button', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const files = page.locator('.activity-btn[aria-label="文件"]')
  const plugins = page.locator('.activity-btn[aria-label="插件"]')
  const from = await files.boundingBox()
  const to = await plugins.boundingBox()
  await plugins.evaluate(e => e.click())
  const result = await plugins.evaluate(e => {
    const a = e.getAnimations({ subtree: true })[0]
    a.pause(); a.currentTime = 0
    const style = getComputedStyle(e, '::before')
    return { x: e.getBoundingClientRect().x, transform: style.transform, width: parseFloat(style.width), fill: style.backgroundColor }
  })
  assert.equal(result.x, to.x)
  assert.ok(Math.abs(result.width - to.width) < 2)
  assert.notEqual(result.fill, 'rgba(0, 0, 0, 0)')
  const dx = await page.evaluate(t => new DOMMatrix(t).e, result.transform)
  assert.ok(Math.abs(to.x + dx - from.x) < 2)
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await files.click()
  assert.equal(await files.evaluate(e => e.getAnimations().length), 0)
  assert.deepEqual(errors, [])
})

test('nested summary numbers use field typography and list items omit the label column', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?complex=1`)
  const cell = page.locator('td.complex-cell').first()
  await cell.waitFor()
  const number = cell.locator('.vc-num').first()
  assert.equal(await number.evaluate(e => getComputedStyle(e).fontSize), '12px')
  await page.evaluate(() => window.motionTest.record())
  const config = page.locator('.inspector-modal .dc-row-foldout').first()
  await config.evaluate(e => e.click())
  const refs = page.locator('.inspector-modal .dc-row-foldout').filter({ hasText: 'References' })
  const x = await refs.evaluate(e => e.getBoundingClientRect().x)
  await refs.evaluate(e => e.click())
  const item = page.locator('.inspector-modal .dc-row-field[data-list-item="true"] > .dc-row-value')
  assert.ok(Math.abs(await item.evaluate(e => e.getBoundingClientRect().x) - x) < 2)
  assert.deepEqual(errors, [])
})


test('file tree selection background travels between rows without clipping', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const rows = page.locator('.file-tree .tree-file:not(.ghost)[aria-selected]')
  await rows.first().waitFor()
  assert.ok(await rows.count() > 1)
  await rows.first().evaluate(e => e.click())
  await rows.first().evaluate(e => e.getAnimations({ subtree: true }).forEach(a => a.finish()))
  const from = await rows.first().boundingBox()
  await rows.nth(1).evaluate(e => e.click())
  const result = await rows.nth(1).evaluate(e => {
    const a = e.getAnimations({ subtree: true })[0]
    a.pause(); a.currentTime = 0
    const p = getComputedStyle(e, '::before')
    return { y: e.getBoundingClientRect().y + new DOMMatrix(p.transform).f, overflow: getComputedStyle(e).overflow, width: parseFloat(p.width), actualWidth: e.getBoundingClientRect().width }
  })
  assert.ok(Math.abs(result.y - from.y) < 2)
  assert.equal(result.overflow, 'visible')
  assert.ok(Math.abs(result.width - result.actualWidth) < 2)
  assert.deepEqual(errors, [])
})

test('initial sidebar background uses theme color before any navigation', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const button = page.locator('.activity-btn.active')
  await button.waitFor()
  const color = await button.evaluate(e => {
    const probe = document.createElement('span'); probe.style.background = 'var(--bg-4)'; e.append(probe)
    const expected = getComputedStyle(probe).backgroundColor; probe.remove()
    return { expected, actual: getComputedStyle(e, '::before').backgroundColor }
  })
  assert.equal(color.actual, color.expected)
  const numeric = page.locator('td:not(.complex-cell) .numeric-value-input').first()
  const alignment = await numeric.evaluate(e => ({ horizontal: getComputedStyle(e).textAlign, vertical: getComputedStyle(e.closest('td')).verticalAlign }))
  assert.equal(alignment.horizontal, 'left')
  assert.equal(alignment.vertical, 'top')
})

test('cell selection frame moves independently and ends on the new cell', async () => {
  const cells = page.locator('td[data-field="name"]')
  await cells.nth(0).click()
  const from = await cells.nth(0).boundingBox()
  await cells.nth(1).dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  const frame = page.locator('.cell-selection-motion')
  const result = await frame.evaluate(e => {
    const a = e.getAnimations()[0]; a.pause(); a.currentTime = 0
    return { x: e.getBoundingClientRect().x, y: e.getBoundingClientRect().y }
  })
  assert.ok(Math.abs(result.x - from.x) < 1)
  assert.ok(Math.abs(result.y - from.y) < 1)
  await frame.evaluate(e => e.getAnimations()[0].finish())
  await frame.waitFor({ state: 'detached' })
  assert.equal(await cells.nth(1).getAttribute('data-cell-motion'), null)
  assert.deepEqual(errors, [])
})


test('overflow preview reveals from cell height without scaling its text', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?overflow=1`)
  const cell = page.locator('td[data-field="name"]').first()
  await cell.waitFor()
  await cell.dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  await page.waitForFunction(() => document.querySelector('td.selected-cell .cell-string-preview')?.getAnimations().some(a => a.effect.getKeyframes()[0].clipPath !== undefined))
  const result = await cell.locator('.cell-string-preview').evaluate(e => {
    const a = e.getAnimations().find(a => a.effect.getKeyframes()[0].clipPath !== undefined); a.pause(); a.currentTime = 0
    const frames = a.effect.getKeyframes()
    return { first: frames[0].clipPath, last: frames.at(-1).clipPath, transform: getComputedStyle(e).transform }
  })
  assert.notEqual(result.first, result.last)
  assert.equal(result.transform, 'none')
  assert.deepEqual(errors, [])
})

test('complex cell gutter selects without toggling its first field', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?complex=1`)
  const cell = page.locator('td.complex-cell').first()
  const tree = cell.locator('.dc-inspector').first()
  const content = await cell.locator('.cell-complex-content').boundingBox()
  const bounds = await tree.boundingBox()
  assert.ok(bounds.x - content.x >= 14)
  const expanded = await cell.locator('[aria-expanded="true"]').count()
  await page.mouse.click(bounds.x - 7, bounds.y + 12)
  assert.equal(await cell.locator('[aria-expanded="true"]').count(), expanded)
  assert.equal(await cell.evaluate(e => e.classList.contains('selected-cell')), true)
})

test('record dragging carries the whole visible row and removes the preview on cancel', async () => {
  const key = page.locator('td.sticky-key-column').first()
  const row = key.locator('..')
  const bounds = await key.boundingBox()
  const rowBounds = await row.boundingBox()
  await page.mouse.move(bounds.x + 18, bounds.y + 14)
  await page.mouse.down()
  await page.mouse.move(bounds.x + 48, bounds.y + 54)
  const preview = page.locator('.record-row-preview')
  await preview.waitFor()
  assert.equal(await preview.locator('td').count(), await row.locator('td').count())
  const moved = await preview.boundingBox()
  assert.ok(moved.width > 240)
  assert.ok(Math.abs(moved.height - rowBounds.height) < 3)
  assert.ok(Math.abs(moved.y - rowBounds.y - 40) < 3)
  await page.evaluate(() => document.dispatchEvent(new PointerEvent('pointercancel')))
  await page.mouse.up()
  assert.equal(await preview.count(), 0)
  assert.equal(await page.locator('.record-row-dragging').count(), 0)
  assert.deepEqual(errors, [])
})

test('view selection background slides with no scale', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const tabs = page.locator('.view-picker [role="tab"]')
  await tabs.nth(1).evaluate(e => e.click())
  const frame = await tabs.nth(1).evaluate(e => {
    const animation = e.getAnimations({ subtree: true }).find(a => a.effect.getKeyframes()[0].transform !== undefined)
    animation.pause(); animation.currentTime = 0
    return animation.effect.getKeyframes()[0].transform
  })
  assert.match(frame, /translate/)
  assert.doesNotMatch(frame, /scale/)
})

test('selecting a cell hidden behind the frozen key column scrolls it into view', async () => {
  const root = page.locator('.table-scroll')
  await root.evaluate(e => { e.style.width = '400px'; e.scrollLeft = 100 })
  const cell = page.locator('td[data-field="name"]').first()
  await cell.dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  const bounds = await cell.boundingBox()
  const frozen = await page.locator('thead .sticky-key-column').boundingBox()
  assert.ok(bounds.x >= frozen.x + frozen.width - 1)
  assert.deepEqual(errors, [])
})

test('source view keeps toolbar height and searches only the current file text', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const toolbar = page.locator('.global-search-bar')
  const height = await toolbar.evaluate(e => e.getBoundingClientRect().height)
  const viewPicker = await page.locator('.view-picker').elementHandle()
  await page.getByRole('tab', { name: '源码', exact: true }).click()
  const search = page.getByRole('searchbox', { name: '搜索本文件文本' })
  await search.waitFor()
  assert.equal(await viewPicker.evaluate(e => e === document.querySelector('.view-picker')), true)
  assert.equal(await toolbar.evaluate(e => e.getBoundingClientRect().height), height)
  const editor = page.locator('.cm-content')
  await editor.fill('Alpha = 1\nalpha = 2\nBeta = 3')
  await search.fill('alpha')
  await page.waitForFunction(() => document.querySelectorAll('.cm-text-search-match').length === 2)
  assert.equal((await page.locator('.global-search-count').textContent()).trim(), '1 / 2')
  await search.press('Enter')
  await page.waitForFunction(() => document.querySelector('.global-search-count').textContent.trim() === '2 / 2')
  assert.equal(await search.evaluate(e => e === document.activeElement), true)
  await search.press('Shift+Enter')
  await page.waitForFunction(() => document.querySelector('.global-search-count').textContent.trim() === '1 / 2')
  await search.fill('missing')
  await page.waitForFunction(() => document.querySelector('.global-search-count').textContent.trim() === '0 / 0')
  await search.press('Escape')
  assert.equal(await page.locator('.cm-text-search-match').count(), 0)
  assert.equal((await editor.textContent()).includes('Beta'), true)
  await editor.click()
  await page.keyboard.press('Control+f')
  assert.equal(await search.evaluate(e => e === document.activeElement), true)
  assert.deepEqual(errors, [])
})

test('rapid cell switches retain one frame and hand off at the exact destination', async () => {
  const cells = page.locator('td[data-field="name"]')
  await cells.nth(0).click()
  await cells.nth(1).dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  const frame = page.locator('.cell-selection-motion')
  const middle = await frame.evaluate(e => {
    const animation = e.getAnimations()[0]; animation.pause(); animation.currentTime = 40
    return e.getBoundingClientRect().toJSON()
  })
  await page.dispatchEvent('body', 'pointerup', { button: 0, pointerId: 1, isPrimary: true })
  await cells.nth(0).dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  assert.equal(await frame.count(), 1)
  const resumed = await frame.evaluate(e => {
    const animation = e.getAnimations()[0]; animation.pause(); animation.currentTime = 0
    return e.getBoundingClientRect().toJSON()
  })
  assert.ok(Math.abs(resumed.y - middle.y) < 1)
  const target = await cells.nth(0).boundingBox()
  const final = await frame.evaluate(e => {
    const animation = e.getAnimations()[0]; animation.currentTime = animation.effect.getTiming().duration
    return e.getBoundingClientRect().toJSON()
  })
  assert.ok(Math.abs(final.x - target.x) < 1)
  assert.ok(Math.abs(final.y - target.y) < 1)
  assert.equal(final.width, resumed.width)
  await frame.evaluate(e => e.getAnimations()[0].finish())
  await frame.waitFor({ state: 'detached' })
  assert.equal(await cells.nth(0).getAttribute('data-cell-motion'), null)
  assert.equal(await cells.nth(0).evaluate(e => getComputedStyle(e, '::after').visibility), 'visible')
  assert.deepEqual(errors, [])
})

test('tab background hands off without leaving an initial offset or an opacity animation', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const tabs = page.locator('.view-picker [role="tab"]')
  await tabs.nth(1).evaluate(e => e.click())
  await tabs.nth(1).evaluate(e => {
    const animation = e.getAnimations({ subtree: true }).find(a => a.effect.getKeyframes()[0].transform !== undefined)
    animation.pause(); animation.currentTime = 60
  })
  await tabs.nth(0).evaluate(e => e.click())
  await tabs.nth(0).evaluate(e => e.getAnimations({ subtree: true }).forEach(a => a.finish()))
  await page.waitForFunction(() => !document.querySelector('.view-picker [data-motion-selection]').style.getPropertyValue('--selection-offset'))
  const result = await tabs.nth(0).evaluate(e => ({ transform: getComputedStyle(e, '::before').transform, opacity: getComputedStyle(e).opacity, animation: getComputedStyle(e).animationName }))
  assert.equal(result.transform, 'none')
  assert.equal(result.opacity, '1')
  assert.equal(result.animation, 'none')
  assert.deepEqual(errors, [])
})

test('expanded cell outline and visible content share exactly the same boundary and timing', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?overflow=1`)
  const cell = page.locator('td[data-field="name"]').first()
  await cell.waitFor()
  await cell.dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  await page.waitForFunction(() => document.querySelector('.cell-selection-motion')?.getAnimations().some(a => a.effect.getKeyframes()[0].height !== undefined))
  const result = await page.evaluate(() => {
    const preview = document.querySelector('td.selected-cell .cell-string-preview')
    const frame = document.querySelector('.cell-selection-motion')
    const content = preview.getAnimations().find(a => a.effect.getKeyframes()[0].clipPath !== undefined)
    const border = frame.getAnimations()[0]
    const sameStart = content.startTime === border.startTime
    content.pause(); border.pause()
    const samples = [0, 45, 90, 135, 180].map(time => {
      content.currentTime = time; border.currentTime = time
      const clip = getComputedStyle(preview).clipPath.match(/-?[\d.]+px/g).map(parseFloat)
      return { border: frame.getBoundingClientRect().bottom, content: preview.getBoundingClientRect().bottom - clip[2] }
    })
    border.finish()
    return { sameStart, samples }
  })
  assert.equal(result.sameStart, true)
  for (const sample of result.samples) assert.ok(Math.abs(sample.border - sample.content) < 1)
  await page.locator('.cell-selection-motion').waitFor({ state: 'detached' })
  assert.equal(await cell.locator('.cell-string-preview').evaluate(e => e.style.clipPath), '')
  assert.equal(await cell.getAttribute('data-cell-motion'), null)
  assert.deepEqual(errors, [])
})

test('switching to an overflowing cell finishes travel before synchronized expansion', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?overflow=1`)
  const cells = page.locator('td[data-field="name"]')
  await cells.nth(1).click()
  await cells.nth(0).dispatchEvent('pointerdown', { button: 0, pointerId: 1, isPrimary: true })
  await page.locator('.cell-selection-motion').evaluate(e => e.getAnimations()[0].pause())
  await page.waitForFunction(() => !!document.querySelector('td.selected-cell .cell-string-preview')?.style.clipPath)
  const preview = cells.nth(0).locator('.cell-string-preview')
  assert.equal(await preview.evaluate(e => e.getAnimations().some(a => a.effect.getKeyframes()[0].clipPath !== undefined)), false)
  await page.locator('.cell-selection-motion').evaluate(e => e.getAnimations()[0].finish())
  await page.waitForFunction(() => document.querySelector('.cell-selection-motion')?.getAnimations().some(a => a.effect.getKeyframes()[0].height !== undefined))
  const synchronized = await page.evaluate(() => {
    const border = document.querySelector('.cell-selection-motion').getAnimations()[0]
    const content = document.querySelector('td.selected-cell .cell-string-preview').getAnimations().find(a => a.effect.getKeyframes()[0].clipPath !== undefined)
    const equal = border.startTime === content.startTime
    border.finish()
    return equal
  })
  assert.equal(synchronized, true)
  await page.locator('.cell-selection-motion').waitFor({ state: 'detached' })
  assert.deepEqual(errors, [])
})


test('diff rows share previews and modal motion while each version has an independent selection', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?extension=diff`)
  const cells = page.locator('td[data-field="Items"]')
  await cells.nth(1).waitFor()
  await cells.nth(0).click({ position: { x: 8, y: 8 } })
  assert.equal(await page.locator('td.selected-cell').count(), 1)
  await page.keyboard.press('ArrowDown')
  assert.ok((await cells.nth(1).getAttribute('class')).includes('selected-cell'))
  assert.equal(await page.locator('td.selected-cell').count(), 1)
  assert.equal(await cells.nth(1).locator('.cell-complex-preview').count(), 1)
  await cells.nth(1).getByRole('button', { name: '放大编辑单元格' }).click()
  await page.locator('.inspector-modal').waitFor()
  assert.equal(await page.locator('.inspector-modal textarea:not([readonly])').count(), 0)
  assert.equal(await page.locator('.inspector-motion-shell').count(), 1)
  await close()
  await page.locator('.inspector-modal').waitFor({ state: 'detached' })
  assert.equal(await page.locator('td.selected-cell').count(), 1)
  const picker = await page.locator('.view-picker').elementHandle()
  const toolbarHeight = await page.locator('.global-search-bar').evaluate(e => e.getBoundingClientRect().height)
  await page.getByRole('tab', { name: '源码', exact: true }).click()
  assert.equal(await picker.evaluate(e => e === document.querySelector('.view-picker')), true)
  assert.equal(await page.locator('.global-search-bar').evaluate(e => e.getBoundingClientRect().height), toolbarHeight)
  await page.getByRole('searchbox', { name: '搜索对比文本' }).fill('match')
  await page.waitForFunction(() => document.querySelectorAll('.cm-text-search-match').length === 3)
  assert.equal(await page.locator('.cm-text-search-current').count(), 1)
  assert.equal(await page.locator('.global-search-count').innerText(), '1 / 3')
  await page.getByRole('button', { name: '下一个匹配' }).click()
  assert.equal(await page.locator('.git-source-pane').nth(1).locator('.cm-text-search-current').count(), 1)
  assert.deepEqual(errors, [])
})

test('dimension complex cells share expansion, overlay editing, reset and stable view tabs', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?extension=dimension`)
  const cell = page.locator('[data-dimension-row="0"][data-dimension-column="2"]')
  await cell.waitFor()
  assert.ok((await cell.getAttribute('class')).includes('complex-cell'))
  await cell.click({ position: { x: 8, y: 8 } })
  assert.equal(await page.locator('td.selected-cell').count(), 1)
  await cell.getByRole('button', { name: '放大编辑单元格' }).click()
  const modal = page.locator('.inspector-modal')
  await modal.waitFor()
  const input = modal.locator('textarea').first()
  await input.fill('override value')
  await close()
  await modal.waitFor({ state: 'detached' })
  assert.equal(await cell.getByRole('button', { name: '清除覆盖，恢复基础值' }).isEnabled(), true)
  await cell.getByRole('button', { name: '清除覆盖，恢复基础值' }).click()
  await page.waitForFunction(() => document.querySelector('[data-dimension-row="0"][data-dimension-column="2"] .dimension-cell-clear').disabled)
  const picker = await page.locator('.view-picker').elementHandle()
  await page.getByRole('tab', { name: '记录', exact: true }).click()
  assert.equal(await picker.evaluate(e => e === document.querySelector('.view-picker')), true)
  const list = page.locator('.dimension-record-list')
  await list.getByRole('button', { name: 'second', exact: true }).click()
  assert.equal(await list.locator('[data-motion-selection]').innerText(), 'second')
  assert.deepEqual(errors, [])
})

test('batch history uses a quiet range and one truthful summary for commit, undo and redo', async () => {
  const count = await page.evaluate(() => window.motionTest.batch())
  const notice = page.locator('.mutation-feedback-notice')
  await page.waitForFunction(count => document.querySelector('.mutation-feedback-notice')?.textContent === `已完成 ${count} 项更改`, count)
  const targets = page.locator('.motion-target-subtle')
  assert.ok(await targets.count() > 1)
  assert.equal(await targets.evaluateAll(elements => elements.every(element => element.getAnimations().length === 0)), true)
  assert.equal(await page.locator('tr.motion-target').count(), 0)
  await page.waitForTimeout(3100)
  assert.equal(await notice.count(), 0)
  await page.evaluate(() => window.motionTest.failedUndo())
  assert.equal(await notice.count(), 0)
  assert.equal(await page.locator('.motion-target').count(), 0)
  await page.evaluate(() => window.motionTest.undo())
  await page.waitForFunction(count => document.querySelector('.mutation-feedback-notice')?.textContent === `已撤销 ${count} 项更改`, count)
  await page.evaluate(() => window.motionTest.redo())
  await page.waitForFunction(count => document.querySelector('.mutation-feedback-notice')?.textContent === `已重做 ${count} 项更改`, count)
  assert.equal(await notice.count(), 1)
  await page.evaluate(() => window.motionTest.clear())
  await notice.waitFor({ state: 'detached' })
  assert.equal(await targets.count(), 0)
  assert.deepEqual(errors, [])
})

test('changing motion preference settles navigation and leaves a static arrival cue', async () => {
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--motion-layout', '2000ms')
    window.motionTest.reveal(5)
  })
  await page.waitForFunction(() => document.getElementById('scroller').scrollTop > 0)
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.waitForFunction(() => document.getElementById('target-5').classList.contains('motion-target'))
  const result = await page.locator('#target-5').evaluate(element => {
    const bounds = element.getBoundingClientRect()
    const scroller = document.getElementById('scroller').getBoundingClientRect()
    return { visible: bounds.top >= scroller.top && bounds.bottom <= scroller.bottom, animations: element.getAnimations().length }
  })
  assert.equal(result.visible, true)
  assert.equal(result.animations, 0)
  assert.deepEqual(errors, [])
})

test('changing motion preference stops an emphasis already in progress without removing its cue', async () => {
  await page.evaluate(() => window.motionTest.commit())
  const cell = page.locator('td[data-field="name"]').first()
  await page.waitForFunction(() => document.querySelector('td[data-field="name"]').getAnimations().length > 0)
  await cell.evaluate(element => element.getAnimations().forEach(animation => animation.pause()))
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.waitForFunction(() => document.querySelector('td[data-field="name"]').getAnimations().length === 0)
  assert.equal(await cell.evaluate(element => element.classList.contains('motion-target')), true)
  assert.deepEqual(errors, [])
})

test('distant selections land immediately and a changing motion preference settles all surfaces', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?surfaces=1`)
  await page.waitForFunction(() => window.surfaceTest)
  await page.evaluate(() => window.surfaceTest.select(1))
  const tabs = page.locator('.view-picker [role="tab"]')
  await tabs.nth(1).evaluate(element => {
    const animation = element.getAnimations({ subtree: true })[0]
    animation.pause(); animation.currentTime = 40
  })
  await page.evaluate(() => window.surfaceTest.select(9))
  assert.equal(await tabs.nth(9).evaluate(element => getComputedStyle(element, '::before').transform), 'none')
  assert.equal(await tabs.nth(9).evaluate(element => element.getAnimations({ subtree: true }).some(animation => animation.effect.getKeyframes()[0].transform !== undefined)), false)
  await page.evaluate(() => window.surfaceTest.select(8))
  await tabs.nth(8).evaluate(element => element.getAnimations({ subtree: true }).forEach(animation => animation.pause()))
  await page.locator('#motion-content').evaluate(element => element.getAnimations().forEach(animation => animation.pause()))
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.waitForFunction(() => !document.querySelector('[aria-selected="true"]').style.getPropertyValue('--selection-offset'))
  assert.equal(await page.locator('#motion-content').evaluate(element => getComputedStyle(element).opacity), '1')
  assert.equal(await page.locator('.icon-spin, .update-icon-spinning').evaluateAll(elements => elements.every(element => getComputedStyle(element).animationName === 'none')), true)
  assert.equal(await tabs.nth(8).getAttribute('aria-selected'), 'true')
  assert.deepEqual(errors, [])
})

test('view cancellation exits once and reduced motion during exit restores the trigger focus', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?surfaces=1`)
  await page.locator('#open-view').click()
  const dialog = page.getByRole('dialog', { name: '新建视图', exact: true })
  await dialog.waitFor()
  await dialog.evaluate(element => {
    const close = element.querySelector('[aria-label="关闭视图设置"]')
    close.click()
    element.getAnimations().forEach(animation => animation.pause())
    close.click()
  })
  assert.equal(await dialog.evaluate(element => element.inert), true)
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 0)
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await dialog.waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 1)
  assert.equal(await page.evaluate(() => document.activeElement.id), 'open-view')
  assert.deepEqual(errors, [])
})

test('view discard confirmation preserves the draft, escapes back and discards only after confirmation', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?surfaces=1`)
  await page.locator('#open-view').click()
  const name = page.getByPlaceholder('输入视图名称')
  await name.fill('保留草稿')
  await name.press('Escape')
  const confirm = page.getByRole('dialog', { name: '放弃更改', exact: true })
  await confirm.waitFor()
  await page.keyboard.press('Escape')
  await page.locator('.view-discard-dialog').waitFor({ state: 'detached' })
  assert.equal(await name.inputValue(), '保留草稿')
  assert.equal(await name.evaluate(element => element === document.activeElement), true)
  await name.press('Escape')
  await confirm.getByRole('button', { name: '继续编辑' }).click()
  await page.locator('.view-discard-dialog').waitFor({ state: 'detached' })
  assert.equal(await name.inputValue(), '保留草稿')
  await name.press('Escape')
  await confirm.getByRole('button', { name: '确认', exact: true }).click()
  await page.locator('.view-editor').waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.surfaceTest.state.submits), 0)
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 1)
  assert.equal(await page.evaluate(() => document.activeElement.id), 'open-view')
  assert.deepEqual(errors, [])
})

test('view save failure retains the editor and retry exits only after a successful save', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?surfaces=1`)
  await page.locator('#open-view').click()
  const dialog = page.getByRole('dialog', { name: '新建视图', exact: true })
  const name = page.getByPlaceholder('输入视图名称')
  await name.fill('测试视图')
  await page.evaluate(() => { window.surfaceTest.state.fail = true; window.surfaceTest.state.delay = 180 })
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  await dialog.getByRole('alert').waitFor()
  assert.equal(await name.inputValue(), '测试视图')
  assert.equal(await dialog.evaluate(element => element.inert), false)
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 0)
  await page.evaluate(() => { window.surfaceTest.state.fail = false })
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await dialog.waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.surfaceTest.state.submits), 2)
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 1)
  assert.deepEqual(errors, [])
})

test('a closing view releases pointer input and never steals a new background editing focus', async () => {
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/motion.html?surfaces=1`)
  await page.evaluate(() => document.documentElement.style.setProperty('--motion-exit', '2000ms'))
  await page.locator('#open-view').click()
  await page.getByRole('button', { name: '关闭视图设置' }).click()
  const surface = page.locator('.view-editor')
  assert.equal(await surface.evaluate(element => element.inert), true)
  const background = page.getByRole('textbox', { name: '背景编辑输入' })
  await background.click()
  await page.keyboard.type('继续输入')
  assert.equal(await background.inputValue(), '继续输入')
  await surface.evaluate(element => element.getAnimations().forEach(animation => animation.finish()))
  await surface.waitFor({ state: 'detached' })
  assert.equal(await background.evaluate(element => element === document.activeElement), true)
  await page.keyboard.type('仍在编辑')
  assert.equal(await background.inputValue(), '继续输入仍在编辑')
  assert.equal(await page.evaluate(() => window.surfaceTest.state.closes), 1)
  assert.deepEqual(errors, [])
})

test('reference entry settles on preference changes and closing preserves new background input', async () => {
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--motion-enter', '2000ms')
    window.motionTest.reference()
  })
  const panel = page.locator('.reference-panel')
  await panel.waitFor()
  await panel.evaluate(element => element.getAnimations().forEach(animation => animation.pause()))
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.waitForFunction(() => document.querySelector('.reference-panel').getAnimations().length === 0)
  assert.equal(await panel.evaluate(element => getComputedStyle(element).transform), 'none')
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  await page.evaluate(() => document.documentElement.style.setProperty('--motion-exit', '2000ms'))
  await page.getByRole('button', { name: '关闭引用记录' }).click()
  await page.locator('.reference-panel[data-motion-exiting="true"]').waitFor()
  const background = page.getByRole('textbox', { name: '引用背景编辑输入' })
  await background.click()
  await page.keyboard.type('引用关闭后继续输入')
  await panel.evaluate(element => element.getAnimations().forEach(animation => animation.finish()))
  await panel.waitFor({ state: 'detached' })
  assert.equal(await background.evaluate(element => element === document.activeElement), true)
  assert.equal(await background.inputValue(), '引用关闭后继续输入')
  assert.deepEqual(errors, [])
})
