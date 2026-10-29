import assert from 'node:assert/strict'
import { after, before, test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { serve } from 'vitepress'
import { chromium } from 'playwright'

let server, browser, origin
before(async () => {
  // 验证真实静态产物，包括无 JavaScript 的首屏；不启动桌面编辑器。
  server = await serve({ root: fileURLToPath(new URL('../docs', import.meta.url)), port: 0 })
  origin = `http://localhost:${server.server.address().port}`
  browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
})
after(async () => {
  await browser?.close()
  if (server?.server) await new Promise((resolve, reject) => server.server.close(error => error ? reject(error) : resolve()))
})

async function assertHero(page) {
  const hero = page.locator('.hero-title')
  await hero.waitFor()
  assert.equal(await hero.evaluate(node => getComputedStyle(node).opacity), '1')
  assert.ok(await hero.evaluate(node => parseFloat(getComputedStyle(node).fontSize) >= 44))
}

test('Chinese and English home pages are styled and visible without JavaScript', async () => {
  for (const [path, text] of [['/', '数据配置工作流'], ['/en/', 'An AI-native data configuration workflow']]) {
    const page = await browser.newPage({ javaScriptEnabled: false, viewport: { width: 390, height: 844 } })
    try {
      await page.goto(origin + path)
      await assertHero(page)
      assert.ok((await page.locator('.hero-title').innerText()).includes(text))
      assert.equal(await page.locator('[data-header]').evaluate(node => getComputedStyle(node).position), 'fixed')
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1))
    } finally { await page.close() }
  }
})

test('a failed formula module leaves content and navigation usable', async () => {
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/examples/chemistry/plugin.js', route => route.abort())
  try {
    await page.goto(origin)
    await page.locator('[data-chemical-output][role=status]').waitFor({ state: 'attached' })
    await assertHero(page)
    assert.equal(await page.locator('[data-chemical-output]').textContent(), '公式演示暂时不可用。')
    await page.locator('[data-lang-trigger]').click()
    assert.equal(await page.locator('[data-lang-trigger]').getAttribute('aria-expanded'), 'true')
    await page.keyboard.press('Escape')
    assert.equal(await page.locator('[data-lang-trigger]').getAttribute('aria-expanded'), 'false')
    const previous = await page.locator('[data-theme-toggle]').getAttribute('aria-pressed')
    await page.locator('[data-theme-toggle]').click()
    assert.notEqual(await page.locator('[data-theme-toggle]').getAttribute('aria-pressed'), previous)
    assert.deepEqual(errors, [])
  } finally { await page.close() }
})

test('a delayed formula module does not block controls and renders current input when ready', async () => {
  const page = await browser.newPage()
  let release
  const pending = new Promise(resolve => { release = resolve })
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/examples/chemistry/plugin.js', async route => { await pending; await route.continue() })
  try {
    await page.goto(origin, { waitUntil: 'domcontentloaded' })
    await page.locator('[data-theme-toggle][aria-pressed]').waitFor()
    await assertHero(page)
    await page.locator('[data-lang-trigger]').click()
    assert.equal(await page.locator('[data-lang-trigger]').getAttribute('aria-expanded'), 'true')
    await page.keyboard.press('Escape')
    await page.locator('[data-chemical-input]').fill('CO2 + H2O')
    release()
    await page.locator('[data-chemical-output] [aria-label="CO2 + H2O"]').waitFor()
    await page.locator('[data-chemical-input]').fill('H2SO4')
    await page.locator('[data-chemical-output] [aria-label="H2SO4"]').waitFor()
    assert.equal(await page.locator('[data-chemical-output] sub').count(), 2)
    assert.deepEqual(errors, [])
  } finally { release(); await page.close() }
})

test('home styles and theme state do not leak into documentation after navigation', async () => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
  const styles = () => page.locator('.vp-doc p').first().evaluate(node => {
    const style = getComputedStyle(node)
    return { margin: style.margin, fontSize: style.fontSize, ink: style.getPropertyValue('--ink'), theme: document.documentElement.dataset.theme }
  })
  try {
    await page.goto(origin + '/docs/')
    await page.locator('.VPNavBarMenuLink').first().waitFor()
    const before = await styles()
    await page.locator('.VPNavBarMenuLink').filter({ hasText: '首页' }).click()
    await page.locator('[data-theme-toggle][aria-pressed]').waitFor()
    await page.locator('[data-theme-toggle]').click()
    await page.goBack()
    await page.locator('.vp-doc').waitFor()
    assert.equal(await page.locator('.prototype-home').count(), 0)
    assert.deepEqual(await styles(), before)
  } finally { await page.close() }
})
