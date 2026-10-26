import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

test('diagnostic center preserves errors and exports details only after explicit selection', async () => {
  const server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, strictPort: false, host: '127.0.0.1' } })
  let browser
  try {
    await server.listen()
    browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
    const page = await browser.newPage()
    const pageErrors = []
    page.on('pageerror', error => pageErrors.push(error.message))
    await page.goto(server.resolvedUrls.local[0])
    await page.getByRole('button', { name: '日志与反馈', exact: true }).waitFor()
    await page.evaluate(() => window.dispatchEvent(new ErrorEvent('error', { message: 'private-project-value', error: new Error('private-project-value') })))
    await page.getByRole('button', { name: /日志与反馈/ }).click()
    await page.getByText(/private-project-value/).first().waitFor()
    await page.getByRole('button', { name: '生成诊断预览' }).click()
    const preview = page.getByRole('textbox', { name: '诊断预览（可编辑脱敏）' })
    assert.ok(!(await preview.inputValue()).includes('private-project-value'))
    await page.getByRole('checkbox', { name: /包含错误文本和堆栈/ }).check()
    await page.getByRole('button', { name: '生成诊断预览' }).click()
    assert.ok((await preview.inputValue()).includes('private-project-value'))
    const downloaded = page.waitForEvent('download')
    await page.getByRole('button', { name: '保存此预览为文件' }).click()
    assert.equal((await downloaded).suggestedFilename(), 'coflow-diagnostics.json')
    assert.deepEqual(pageErrors, [])
  } finally { await browser?.close(); await server.close() }
})
