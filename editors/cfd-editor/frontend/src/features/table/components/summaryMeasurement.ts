/** 摘要测量统一分成准备、读取和提交三个阶段，避免单元格之间反复触发布局。 */
interface SummaryJob {
  root: HTMLElement
  total: number
  updateWindow: (visible: number, prepared: number) => void
}
const jobs = new Map<HTMLElement, SummaryJob>()
const pending = new Set<HTMLElement>()
const widths = new Map<string, number>()
let frame = 0
let observer: ResizeObserver | undefined
let mutations: MutationObserver | undefined
let styleObserver: MutationObserver | undefined

function remember(key: string, width: number) {
  // 文本缓存有界；代际切换后旧字符串不会无限积累。
  widths.set(key, width)
}
function schedule(root: HTMLElement) {
  pending.add(root)
  if (!frame) frame = requestAnimationFrame(flush)
}
function invalidate() {
  widths.clear()
  for (const root of jobs.keys()) schedule(root)
}
function flush() {
  frame = 0
  const batch = [...pending].flatMap(root => {
    const job = jobs.get(root)
    if (!job || !root.isConnected) return []
    const style = getComputedStyle(root)
    const logicalWidth = parseFloat(style.width) || root.clientWidth
    const visualWidth = root.getBoundingClientRect().width
    // 预加载视图可能处于 display:none；零尺寸不能污染跨单元格共享的宽度缓存。
    if (logicalWidth <= 0 || visualWidth <= 0) return []
    // rect 是视觉像素，计算始终使用 CSS 逻辑像素，包括多层 zoom。
    const zoom = visualWidth / logicalWidth
    const parts = [...root.querySelectorAll<HTMLElement>(':scope > .dc-summary-part')]
    const rest = root.querySelector<HTMLElement>(':scope > .dc-summary-rest')!
    const signature = `${root.closest('.dc-inspector')?.className}|${style.font}|${style.letterSpacing}|`
    return [{ job, logicalWidth, zoom, gap: parseFloat(style.columnGap) || 0, parts, rest, signature,
      keys: parts.map(part => signature + part.innerHTML), probes: [] as { node: HTMLElement; key: string; remaining: number }[] }]
  })
  pending.clear()
  const preparedCounts = new Set<string>()
  // 统一写入：只显示尚未测量的片段，计数探针在原样式上下文中使用绝对定位。
  for (const entry of batch) {
    entry.parts.forEach((part, index) => { if (!widths.has(entry.keys[index])) part.hidden = false })
    for (let visible = 0; visible <= entry.parts.length; visible++) {
      const remaining = entry.job.total - visible
      if (!remaining) continue
      const key = `${entry.signature}rest:${remaining}`
      if (widths.has(key) || preparedCounts.has(key)) continue
      preparedCounts.add(key)
      const node = entry.rest.cloneNode(false) as HTMLElement
      node.hidden = false
      node.textContent = `+${remaining}`
      node.style.cssText = 'position:absolute;visibility:hidden;pointer-events:none'
      entry.job.root.append(node)
      entry.probes.push({ node, key, remaining })
    }
  }
  // 统一读取：所有 DOM 写入完成后才读取精确尺寸，后续匹配为纯数值计算。
  for (const entry of batch) {
    entry.parts.forEach((part, index) => {
      if (!widths.has(entry.keys[index])) remember(entry.keys[index], part.getBoundingClientRect().width / entry.zoom)
    })
    entry.probes.forEach(probe => remember(probe.key, probe.node.getBoundingClientRect().width / entry.zoom))
  }
  for (const entry of batch) {
    const prefix = [0]
    entry.keys.forEach(key => prefix.push(prefix[prefix.length - 1] + (widths.get(key) ?? 0)))
    let visible = entry.parts.length
    for (; visible > 0; visible--) {
      const remaining = entry.job.total - visible
      const restWidth = remaining ? widths.get(`${entry.signature}rest:${remaining}`)! : 0
      if (prefix[visible] + entry.gap * Math.max(0, visible - 1)
        + (remaining ? restWidth + entry.gap : 0) <= entry.logicalWidth + 0.01) break
    }
    entry.parts.forEach((part, index) => { part.hidden = index >= visible })
    const remaining = entry.job.total - visible
    entry.rest.hidden = !remaining
    entry.rest.textContent = `+${remaining}`
    entry.probes.forEach(probe => probe.node.remove())
    entry.job.updateWindow(visible, entry.parts.length)
  }
  if (widths.size > 4096) {
    const excess = widths.size - 4096
    const keys = widths.keys()
    for (let index = 0; index < excess; index++) widths.delete(keys.next().value!)
  }
}

export function observeSummary(root: HTMLElement, total: number, updateWindow: SummaryJob['updateWindow']): () => void {
  if (!observer) {
    observer = new ResizeObserver(entries => entries.forEach(entry => schedule(entry.target as HTMLElement)))
    mutations = new MutationObserver(entries => {
      for (const entry of entries) {
        const element = entry.target instanceof Element ? entry.target : entry.target.parentElement
        // 只监听片段正文；计数和测量探针的提交不能反过来触发测量。
        const part = element?.closest('.dc-summary-part')
        const root = part?.parentElement
        if (root && jobs.has(root)) schedule(root)
      }
    })
    mutations.observe(document.body, { childList: true, subtree: true, characterData: true })
    styleObserver = new MutationObserver(invalidate)
    styleObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] })
    styleObserver.observe(document.body, { attributes: true, attributeFilter: ['class', 'style'] })
    document.fonts.addEventListener('loadingdone', invalidate)
  }
  jobs.set(root, { root, total, updateWindow })
  observer.observe(root)
  schedule(root)
  return () => {
    jobs.delete(root)
    pending.delete(root)
    observer!.unobserve(root)
    if (!jobs.size) {
      observer!.disconnect(); observer = undefined
      mutations!.disconnect()
      mutations = undefined
      styleObserver!.disconnect(); styleObserver = undefined
      document.fonts.removeEventListener('loadingdone', invalidate)
      cancelAnimationFrame(frame); frame = 0
    }
  }
}
