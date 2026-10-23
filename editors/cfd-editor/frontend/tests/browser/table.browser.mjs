import assert from 'node:assert/strict'
import { after, before, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
const input = (row, field = 'name') => page.locator(`tr[data-coordinate-id]`).filter({ has: page.locator(`[data-table-value-cell][data-field="${field}"]`) })
  .locator(`[data-field="${field}"] textarea`).nth(row)
const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, strictPort: false, host: '127.0.0.1' } })
  await server.listen()
  browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
  page = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  page.setDefaultTimeout(5000)
  page.on('pageerror', error => { errors.push(error.message); console.error(error.message) })
})
after(async () => { await browser?.close(); await server?.close() })
beforeEach(async () => {
  errors.length = 0
  await page.goto(`${server.resolvedUrls.local[0]}tests/browser/table.html`)
  await input(0).waitFor()
  await settle()
})

test('single click edits the existing string node without row resize or scroll', async () => {
  const field = input(4)
  const node = await field.elementHandle()
  const before = await field.boundingBox()
  await field.click()
  await settle()
  assert.equal(await field.evaluate((element, original) => element === original && element === document.activeElement, node), true)
  assert.equal(await page.locator('#selection').textContent(), 'value:row_4')
  assert.equal((await field.boundingBox()).height, before.height)
  assert.equal(await page.locator('.table-scroll').evaluate(el => el.scrollTop), 0)
  assert.deepEqual(errors, [])
})

test('long text is truncated until focus and expands downwards without changing column width', async () => {
  const field = input(2)
  const before = await field.boundingBox()
  await field.click()
  await settle()
  const expanded = await field.boundingBox()
  assert.ok(expanded.height > before.height * 2)
  assert.equal(expanded.width, before.width)
  assert.equal(expanded.y, before.y)
  await page.locator('#outside').click()
  await settle()
  assert.equal((await field.boundingBox()).height, before.height)
})

test('clicking string cell padding focuses the same input immediately', async () => {
  await page.locator('td[data-coordinate-id="Item::row_1"][data-field="name"]').click({ position: { x: 3, y: 3 } })
  assert.equal(await input(1).evaluate(el => el === document.activeElement), true)
  assert.equal(await page.locator('#selection').textContent(), 'value:row_1')
})

test('snapshot updates preserve the input, caret and newer draft; blur never steals focus', async () => {
  const field = input(0)
  await field.click()
  const node = await field.elementHandle()
  const width = (await field.boundingBox()).width
  await field.fill('保存内容')
  await field.press('Enter')
  await field.press('End')
  await field.pressSequentially('后续草稿')
  await page.waitForFunction(() => window.tableTest.writes === 1)
  await page.waitForTimeout(100)
  assert.equal(await field.inputValue(), '保存内容后续草稿')
  assert.equal(await field.evaluate((element, original) => element === original && element === document.activeElement, node), true)
  assert.equal((await field.boundingBox()).width, width)
  await input(1).click()
  await page.waitForTimeout(100)
  assert.equal(await input(1).evaluate(el => el === document.activeElement), true)
  assert.equal(await page.locator('#selection').textContent(), 'value:row_1')
  assert.equal(await page.evaluate(() => window.tableTest.writes), 2)
  assert.deepEqual(errors, [])
})

test('Escape cancels and IME confirmation does not commit', async () => {
  const field = input(0)
  await field.fill('临时草稿')
  await field.press('Escape')
  assert.equal(await field.inputValue(), '文本 0')
  await field.fill('中文输入')
  await field.dispatchEvent('keydown', { key: 'Enter', code: 'Enter', isComposing: true })
  await page.waitForTimeout(60)
  assert.equal(await page.evaluate(() => window.tableTest.writes), 0)
  await field.press('Enter')
  await page.waitForTimeout(60)
  assert.equal(await page.evaluate(() => window.tableTest.writes), 1)
})

test('Shift click selects a range without moving focus to the target editor', async () => {
  await input(0).click()
  await input(1).click({ modifiers: ['Shift'] })
  assert.equal(await page.locator('td.selected-cell').count(), 2)
  assert.equal(await input(1).evaluate(el => el === document.activeElement), false)
  assert.equal(await page.locator('.table-scroll').evaluate(el => el.scrollTop), 0)
})

test('failed saves retain the draft and expose an inline error', async () => {
  await page.evaluate(() => { window.tableTest.fail = true })
  await input(0).fill('不能丢失的草稿')
  await input(1).click()
  await page.getByRole('alert').waitFor()
  assert.equal(await input(0).inputValue(), '不能丢失的草稿')
  assert.equal(await input(1).evaluate(el => el === document.activeElement), true)
  assert.deepEqual(errors, [])
})

test('dimension inheritance becomes an override without remounting the string input', async () => {
  const field = page.locator('#dimension textarea')
  const node = await field.elementHandle()
  await field.fill('翻译')
  await field.press('Enter')
  await page.locator('#dimension [aria-label="已覆盖基础值"]').waitFor()
  assert.equal(await field.evaluate((element, original) => element === original && element === document.activeElement, node), true)
  await page.getByRole('button', { name: '清除覆盖，恢复基础值' }).click()
  await page.locator('#dimension [aria-label="继承基础值"]').waitFor()
  assert.equal(await field.inputValue(), '基础文本')
  assert.deepEqual(errors, [])
})

test('keyboard navigation reveals virtual rows, and typing uses the existing string input', async () => {
  await input(0).click()
  await page.locator('.table-scroll').focus()
  for (let index = 0; index < 24; index++) await page.keyboard.press('ArrowDown')
  await settle()
  assert.equal(await page.locator('#selection').textContent(), 'value:row_24')
  assert.ok(await page.locator('.table-scroll').evaluate(el => el.scrollTop > 0))
  const field = page.locator('[data-coordinate-id="Item::row_24"][data-field="name"] textarea')
  const node = await field.elementHandle()
  await page.keyboard.press('x')
  assert.equal(await field.inputValue(), 'x')
  assert.equal(await field.evaluate((element, original) => element === original && element === document.activeElement, node), true)
})

test('drag selection stays on the hit cell and does not repeatedly publish the same selection', async () => {
  const first = page.locator('td[data-coordinate-id="Item::row_0"][data-field="name"]')
  const last = page.locator('td[data-coordinate-id="Item::row_4"][data-field="icon"]')
  const a = await first.boundingBox(), b = await last.boundingBox()
  // 从单元格内边距起拖，文字区域继续保留原生文本选择。
  await page.mouse.move(a.x + 3, a.y + 3)
  await page.mouse.down()
  await page.mouse.move(b.x + 3, b.y + 3, { steps: 12 })
  const count = await page.evaluate(() => window.tableTest.selectionEvents.length)
  await page.mouse.move(b.x + 4, b.y + 4, { steps: 5 })
  assert.equal(await page.evaluate(() => window.tableTest.selectionEvents.length), count)
  await page.mouse.up()
  assert.equal(await page.locator('td.selected-cell').count(), 10)
  assert.equal(await page.locator('.table-scroll').evaluate(el => el.scrollTop), 0)
})

test('workspace selection and inspector updates preserve direct text focus', async () => {
  await page.goto(server.resolvedUrls.local[0])
  const field = page.locator('.data-table .cell-string-textarea').first()
  await field.waitFor()
  await field.click()
  await settle()
  assert.equal(await field.evaluate(el => el === document.activeElement), true)
  await field.fill('工作区输入验证')
  // 浏览器示例没有写入后端；异步保存由独立快照测试覆盖。
  await settle()
  assert.equal(await field.inputValue(), '工作区输入验证')
  assert.equal(await field.evaluate(el => el === document.activeElement), true)
  assert.deepEqual(errors, [])
  for (const theme of ['dark', 'light']) {
    await page.locator('html').evaluate((el, value) => el.dataset.theme = value, theme)
    await page.screenshot({ path: join(tmpdir(), `coflow-ui-${theme}.png`), animations: 'disabled' })
  }
})
