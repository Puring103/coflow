import assert from 'node:assert/strict'
import { after, before, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, strictPort: false, host: '127.0.0.1' } })
  await server.listen()
  browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
  page = await browser.newPage({ viewport: { width: 1400, height: 1080 }, reducedMotion: 'reduce' })
  page.setDefaultTimeout(5000)
  page.on('pageerror', error => errors.push(error.message))
})
after(async () => { await browser?.close(); await server?.close() })
beforeEach(async () => {
  errors.length = 0
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/feedback-notice.html`)
  await page.waitForFunction(() => window.motionTest && window.noticeTest && document.querySelector('.feedback-notice'))
})

test('clicking a notice over the modal backdrop preserves edits, focus and selection', async () => {
  await page.evaluate(() => window.motionTest.open())
  const panel = page.locator('.inspector-modal')
  await panel.waitFor()
  await page.waitForFunction(() => document.querySelector('.inspector-modal .keyboard-selected'))
  const focused = await page.evaluateHandle(() => document.activeElement)
  const selected = await panel.locator('.keyboard-selected').evaluateAll(elements => elements.map(element => element.getAttribute('data-field-path-wire')))
  const notice = page.locator('.feedback-notice')
  const noticeBounds = await notice.boundingBox()
  const panelBounds = await panel.boundingBox()
  assert.ok(noticeBounds.y > panelBounds.y + panelBounds.height, 'the regression must click over the actual backdrop')
  await notice.click()
  assert.equal(await panel.count(), 1)
  assert.deepEqual(await page.evaluate(() => ({ commits: window.motionTest.control.commits, closes: window.motionTest.control.closes })), { commits: 0, closes: 0 })
  assert.equal(await page.evaluate(element => document.activeElement === element, focused), true)
  assert.deepEqual(await panel.locator('.keyboard-selected').evaluateAll(elements => elements.map(element => element.getAttribute('data-field-path-wire'))), selected)
  assert.deepEqual(errors, [])
  await focused.dispose()
})

test('notices preserve table selection while the surrounding empty space still dismisses it', async () => {
  await page.locator('td[data-field="name"]').first().click()
  const selected = page.locator('td[data-field="name"].selected-cell')
  await selected.waitFor()
  await page.locator('.feedback-notice').click()
  assert.equal(await selected.count(), 1)
  await page.locator('.table-scroll').click({ position: { x: 830, y: 350 } })
  assert.equal(await selected.count(), 0)
  assert.deepEqual(errors, [])
})

test('StrictMode shares one stacked region and removes it after the last notice', async () => {
  await page.evaluate(() => window.noticeTest.setCount(2))
  await page.waitForFunction(() => document.querySelectorAll('.feedback-notice').length === 2)
  assert.equal(await page.locator('.editor-notices').count(), 1)
  const rects = await page.locator('.feedback-notice').evaluateAll(elements => elements.map(element => {
    const rect = element.getBoundingClientRect()
    return { top: rect.top, bottom: rect.bottom }
  }))
  assert.ok(rects[0].bottom < rects[1].top)
  await page.evaluate(() => window.noticeTest.setCount(1))
  await page.waitForFunction(() => document.querySelectorAll('.feedback-notice').length === 1)
  assert.equal(await page.locator('.editor-notices').count(), 1)
  await page.evaluate(() => window.noticeTest.setCount(0))
  await page.waitForFunction(() => document.querySelector('.editor-notices') === null)
  assert.deepEqual(errors, [])
})
