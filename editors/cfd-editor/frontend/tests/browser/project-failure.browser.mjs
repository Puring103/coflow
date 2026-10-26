import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright'

test('failed project load keeps Git diff, tabs and navigation usable', async () => {
  const server = await createServer({ root: fileURLToPath(new URL('../..', import.meta.url)), server: { port: 0, strictPort: false, host: '127.0.0.1' } })
  let browser
  try {
    await server.listen()
    browser = await chromium.launch({ headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined })
    const page = await browser.newPage()
    const errors = []
    page.on('pageerror', error => errors.push(error.message))
    // 模拟桌面 IPC 的项目加载失败，实际挂载完整 App，覆盖空会话点击路径。
    await page.addInitScript(() => {
      localStorage.setItem('cfd-editor-last-project-yaml', 'missing/coflow.yaml')
      window.diffRequests = 0
      window.__TAURI_INTERNALS__ = {
        transformCallback: () => 1,
        unregisterCallback() {},
        async invoke(command) {
          if (command === 'load_project') throw new Error('项目配置不存在')
          if (command === 'get_project_diff') { window.diffRequests++; throw new Error('无效会话') }
          if (command === 'list_frontend_plugins') return { plugins: [], errors: [] }
          if (command === 'plugin:app|version') return '0.13.0'
          if (command === 'plugin:event|listen') return 1
          return null
        },
      }
    })
    await page.goto(server.resolvedUrls.local[0])
    await page.getByText('自动打开上次项目失败: 项目配置不存在', { exact: true }).waitFor()
    for (let attempt = 0; attempt < 3; attempt++) {
      await page.getByRole('button', { name: 'Git 变更', exact: true }).click()
      await page.getByText('请先成功打开项目后查看 Git 变更', { exact: true }).waitFor()
      await page.getByRole('button', { name: '插件', exact: true }).click()
      await page.getByRole('button', { name: '文件', exact: true }).click()
      await page.getByRole('tab', { name: 'Git Diff' }).click()
      await page.getByText('请先成功打开项目后查看 Git 变更', { exact: true }).waitFor()
    }
    assert.equal(await page.evaluate(() => window.diffRequests), 0)
    assert.deepEqual(errors, [])
  } finally {
    await browser?.close()
    await server.close()
  }
})
