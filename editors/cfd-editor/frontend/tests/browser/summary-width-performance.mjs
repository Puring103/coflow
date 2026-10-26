import assert from 'node:assert/strict'
import { chromium } from 'playwright'
import { writeFileSync } from 'node:fs'
import { cpus } from 'node:os'
import { resolve } from 'node:path'

// 独立排版微基准：对比逐摘要读写、批量精确测量与已缓存宽度的计算。
// 使用简化片段，不以此代替真实字段树的端到端滚动验收。
const browser = await chromium.launch({ headless: true })
const results = []
try {
  for (let round = 0; round < 3; round++) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
    await page.setContent('<style>body{font:12px "Segoe UI","Microsoft YaHei",sans-serif}.root{display:flex;gap:12px;overflow:hidden;white-space:nowrap;height:24px}.part{display:inline-flex;gap:4px;flex:none}.part[hidden],.rest[hidden]{display:none}.key{font-size:11px}.rest{font-size:11px;flex:none}.ruler{position:absolute;visibility:hidden;display:flex;gap:8px}.ruler span{font-size:11px;flex:none}</style><main></main>')
    results.push(await page.evaluate(round => {
      const widths = Array.from({ length: 200 }, (_, index) => 230 + index % 19 * 10)
      const main = document.querySelector('main')
      const render = () => {
        main.innerHTML = widths.map((width, index) => `<div class="root" style="width:${width}px">${Array.from({ length: 8 }, (_, item) => `<span class="part"><span class="key">字段${item}:</span><span>项目${index % 7}_${item}</span></span>`).join('')}<span class="rest" hidden></span></div>`).join('')
        // 固定每个方法开始前的 DOM 状态，计时包含方法自己造成的布局。
        void main.offsetHeight
        return [...main.children].map(root => ({ root, parts: [...root.querySelectorAll('.part')], rest: root.querySelector('.rest') }))
      }
      let rows = render()
      let started = performance.now()
      const oldCounts = rows.map(({ root, parts, rest }) => {
        parts.forEach(part => { part.hidden = false })
        const prefix = [0]
        parts.forEach(part => prefix.push(prefix.at(-1) + part.getBoundingClientRect().width))
        let count = parts.length
        while (count >= 0) {
          const remaining = parts.length - count
          rest.hidden = remaining === 0
          rest.textContent = `+${remaining}`
          const used = prefix[count] + 12 * Math.max(0, count - 1)
            + (remaining ? rest.getBoundingClientRect().width + (count ? 12 : 0) : 0)
          if (used <= root.clientWidth || count === 0) break
          count--
        }
        parts.forEach((part, index) => { part.hidden = index >= count })
        return count
      })
      const oldMs = performance.now() - started
      rows = render()
      started = performance.now()
      const ruler = document.createElement('div')
      ruler.className = 'ruler'
      ruler.innerHTML = Array.from({ length: 8 }, (_, index) => `<span>+${index + 1}</span>`).join('')
      document.body.append(ruler)
      // 全部样式写入完成后集中读取；余项宽度只按不同文本测一次。
      const suffix = [0, ...[...ruler.children].map(element => element.getBoundingClientRect().width)]
      const measurements = rows.map(({ root, parts }) => {
        const prefix = [0]
        parts.forEach(part => prefix.push(prefix.at(-1) + part.getBoundingClientRect().width))
        return { prefix, width: root.clientWidth }
      })
      function fit(prefix, width) {
        const total = prefix.length - 1
        // 不假设包含动态 +n 的谓词单调，按降序取第一个可容纳数量。
        for (let count = total; count >= 0; count--) {
          const remaining = total - count
          if (prefix[count] + 12 * Math.max(0, count - 1)
            + suffix[remaining] + (remaining && count ? 12 : 0) <= width || count === 0) return count
        }
      }
      const apply = counts => rows.forEach(({ parts, rest }, row) => {
        const count = counts[row]
        rest.hidden = count === parts.length
        rest.textContent = `+${parts.length - count}`
        parts.forEach((part, index) => { part.hidden = index >= count })
      })
      const batchCounts = measurements.map(({ prefix, width }) => fit(prefix, width))
      apply(batchCounts)
      const batchMs = performance.now() - started
      started = performance.now()
      // 宽度已缓存、容器宽度已知时，热点路径只有数值计算和最终写入。
      const cachedCounts = measurements.map(({ prefix, width }) => fit(prefix, width))
      apply(cachedCounts)
      const cachedMs = performance.now() - started
      // 同时核对缩放后的坐标单位，避免把视觉像素宽度与 CSS 容器宽度混用。
      main.style.zoom = '1.25'
      const zoomGeometry = { zoom: 1.25, containerClientWidth: rows[0].root.clientWidth,
        containerRectWidth: rows[0].root.getBoundingClientRect().width,
        partRectWidth: rows[0].parts[0].getBoundingClientRect().width,
        partLogicalWidth: measurements[0].prefix[1] }
      return { round, rows: 200, partsPerRow: 8, oldMs, batchMs, cachedMs,
        zoomGeometry,
        equivalent: JSON.stringify(oldCounts) === JSON.stringify(batchCounts) && JSON.stringify(oldCounts) === JSON.stringify(cachedCounts) }
    }, round))
    assert.ok(results.at(-1).equivalent)
    console.log(JSON.stringify(results.at(-1)))
    await page.close()
  }
  const output = resolve(process.env.SUMMARY_WIDTH_OUTPUT ?? 'summary-width-performance.json')
  writeFileSync(output, JSON.stringify({ cpu: cpus()[0].model, browser: browser.version(), results }, null, 2))
} finally {
  await browser.close()
}
