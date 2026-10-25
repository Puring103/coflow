import assert from 'node:assert/strict'
import { before, after, beforeEach, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

let server, browser, page
const errors = []
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, host: '127.0.0.1' } })
  await server.listen()
  browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 } })
  page.on('pageerror', error => errors.push(error.message))
})
after(async () => { await browser?.close(); await server?.close() })
beforeEach(async () => {
  errors.length = 0
  await page.goto(`${server.resolvedUrls.local[0]}previews/cells.html`)
  await page.locator('[data-sample="basics"] textarea').first().waitFor()
})
const row = (sample, label) => page.locator(`[data-sample="${sample}"] .dc-row-field`).filter({ has: page.locator('.dc-row-label-text', { hasText: new RegExp(`^${label}$`) }) }).first()

test('preview edits existing scalar controls while keeping labels and read-only values', async () => {
  assert.equal(await page.locator('[data-sample]').count(), 8)
  const field = row('basics', '初始水量')
  await field.locator('textarea').fill('72')
  await field.locator('textarea').press('Enter')
  assert.equal(await field.locator('textarea').inputValue(), '72')
  assert.equal(await field.locator('.dc-row-label-text').textContent(), '初始水量')
  assert.equal(await row('readonly', 'BaseValue').locator('textarea').count(), 0)
  assert.ok((await row('readonly', 'BaseValue').getAttribute('title')).includes('只读'))
  await row('basics', '启用').locator('input[type="checkbox"]').uncheck()
  assert.equal(await row('basics', '启用').locator('input').isChecked(), false)
  await row('basics', '品质').locator('input').click()
  await page.getByRole('option', { name: 'Common', exact: true }).click()
  assert.equal(await row('basics', '品质').locator('input').inputValue(), 'Common')
  await row('basics', '引用模板').getByRole('button', { name: '打开引用记录' }).click()
  assert.equal(await page.locator('.reference-panel').count(), 1)
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  assert.deepEqual(errors, [])
})

test('deep mixed nesting edits the correct leaf and survives collapse and re-expansion', async () => {
  await page.getByRole('button', { name: '展开全部', exact: true }).click()
  const weight = row('nested', 'Weight')
  const label = await weight.locator('.dc-row-label').boundingBox()
  await weight.locator('textarea').fill('81')
  const bounds = await weight.locator('textarea').boundingBox()
  assert.ok(bounds.x >= label.x + label.width - 1)
  await weight.locator('textarea').press('Enter')
  await page.getByRole('button', { name: '收起全部', exact: true }).click()
  assert.equal(await row('nested', 'Weight').count(), 0)
  await page.getByRole('button', { name: '展开全部', exact: true }).click()
  assert.equal(await row('nested', 'Weight').locator('textarea').inputValue(), '81')
  assert.equal(await row('nested', 'Weight').nth(0).locator('.dc-row-label-text').textContent(), 'Weight')
  assert.equal(await page.locator('[role="status"]').textContent(), '已保存 1 次修改')
  assert.deepEqual(errors, [])
})

test('empty states and multiline fading remain distinct in both themes', async () => {
  assert.equal(await page.locator('[data-sample="states"] .empty-preview').textContent(), '空字符串')
  await page.getByRole('button', { name: '展开全部', exact: true }).click()
  assert.ok((await page.locator('[data-sample="states"]').innerText()).includes('空数组'))
  assert.ok((await page.locator('[data-sample="states"]').innerText()).includes('空字典'))
  const preview = page.locator('[data-sample="text"] .cell-string-preview')
  await page.locator('[data-sample="text"] .cell-string-overflow').waitFor()
  assert.ok((await preview.evaluate(element => getComputedStyle(element).maskImage)).includes('linear-gradient'))
  await page.getByRole('button', { name: '浅色外观' }).click()
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'light')
  assert.deepEqual(errors, [])
})
