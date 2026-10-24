import { createServer } from '../../editors/cfd-editor/frontend/node_modules/vite/dist/node/index.js'
import { chromium } from '../../editors/cfd-editor/frontend/node_modules/playwright/index.mjs'
import { fileURLToPath } from 'node:url'
import { mkdir, readFile } from 'node:fs/promises'
import assert from 'node:assert/strict'

// 只启动无头前端预览，不启动或操作桌面编辑器进程。
const frontend = fileURLToPath(new URL('../../editors/cfd-editor/frontend/', import.meta.url))
const output = fileURLToPath(new URL('../docs/public/assets/', import.meta.url))
// 校验截图数据与可运行的 CFD 案例一致，避免官网视图各自演变。
const records = JSON.parse(await readFile(new URL('../../editors/cfd-editor/frontend/tests/browser/home-skills.json', import.meta.url), 'utf8'))
const expected = records.map(([id, name, element, tier, power, cooldown, prerequisites]) =>
  `${id}: Skill {\n  name: "${name}", element: Element::${element}, tier: ${tier},\n  power: ${power}, cooldown: ${cooldown.toFixed(1)}, prerequisites: [${prerequisites.map(id => `&Skill::${id}`).join(', ')}],\n}`,
).join('\n') + '\n'
assert.equal((await readFile(new URL('../docs/public/examples/skill-tree/data/skills.cfd', import.meta.url), 'utf8')).replaceAll('\r\n', '\n'), expected)
const server = await createServer({ root: frontend, server: { host: '127.0.0.1', port: 0, strictPort: false } })
let browser
try {
  await server.listen()
  browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 960, height: 500 }, deviceScaleFactor: 2 })
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  await mkdir(output, { recursive: true })
  for (const theme of ['light', 'dark']) {
    await page.goto(`${server.resolvedUrls.local[0]}tests/browser/home-graph.html?theme=${theme}`)
    await page.waitForFunction(() => document.querySelectorAll('.react-flow__node').length === 6)
    await page.waitForFunction(() => document.querySelectorAll('.react-flow__edge').length === 6)
    await page.getByText('元素精通', { exact: true }).waitFor()
    await page.evaluate(() => document.fonts.ready)
    await page.waitForTimeout(800)
    await page.screenshot({ path: `${output}/skill-tree-${theme}.png` })
  }
  if (errors.length) throw new Error(errors.join('\n'))
  console.log('Captured real GraphView: six nodes and six references, light and dark.')
} finally {
  await browser?.close()
  await server.close()
}
