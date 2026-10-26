/** CSS 是动效参数的唯一来源，脚本动画与样式使用相同的时长和曲线。 */
export function motionToken(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(`--motion-${name}`).trim()
}

export function motionDuration(name: string): number {
  return Number.parseFloat(motionToken(name))
}

export function reducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

export interface MotionOrigin {
  resolve: () => HTMLElement | null
  focus: () => void
  changed?: boolean
}

/** 保留单元格身份而不是旧矩形；虚拟行重新挂载后仍能找到同一来源。 */
export function cellMotionOrigin(element: HTMLElement): MotionOrigin {
  const cell = element.closest<HTMLElement>('[data-table-cell-key]')!
  const root = cell.closest<HTMLElement>('.table-scroll')!
  const key = cell.dataset.tableCellKey!
  const resolve = () => root.isConnected
    ? root.querySelector<HTMLElement>(`[data-table-cell-key="${CSS.escape(key)}"]`) : null
  return { resolve, focus: () => { if (root.isConnected) root.focus({ preventScroll: true }) } }
}

/** 来源按可见表面裁切，排除粘性表头和冻结列遮挡的区域。 */
export function originBounds(origin: MotionOrigin | null | undefined): DOMRect | null {
  const cell = origin?.resolve()
  if (!cell?.isConnected || !cell.getClientRects().length) return null
  const root = cell.closest<HTMLElement>('.table-scroll')
  const expanded = cell.querySelector<HTMLElement>(cell.classList.contains('complex-cell')
    ? '.cell-complex-preview.overflowing > .cell-complex-viewport > .cell-complex-content'
    : '.cell-string-input.focused > .cell-string-textarea, .cell-string-input:not(.focused) > .cell-string-preview')
  const surface = cell.matches('.selected-cell') && expanded ? expanded : cell
  const rect = surface.getBoundingClientRect()
  const viewport = root?.getBoundingClientRect()
  const headers = root ? [...root.querySelectorAll<HTMLElement>('thead th')] : []
  const frozen = cell.classList.contains('sticky-key-column') ? null : root?.querySelector('thead .sticky-key-column')
  const left = Math.max(0, rect.left, viewport?.left ?? 0, frozen?.getBoundingClientRect().right ?? 0)
  const top = Math.max(0, rect.top, viewport?.top ?? 0, ...headers.map(item => item.getBoundingClientRect().bottom))
  const right = Math.min(innerWidth, rect.right, viewport?.right ?? innerWidth)
  const bottom = Math.min(innerHeight, rect.bottom, viewport?.bottom ?? innerHeight)
  return right > left && bottom > top ? new DOMRect(left, top, right - left, bottom - top) : null
}

const emphasis = new WeakMap<HTMLElement, () => void>()

/** 强调在目标到达后开始；重复定位会替换旧反馈，减少动态模式只保留静态提示。 */
export function emphasize(element: HTMLElement): () => void {
  emphasis.get(element)?.()
  const background = getComputedStyle(element).backgroundColor
  const highlighted = `color-mix(in srgb, var(--accent) 22%, ${background})`
  element.classList.add('motion-target')
  const animation = !reducedMotion() ? element.animate([
    { backgroundColor: highlighted, outlineColor: 'var(--accent)', offset: 0 },
    { backgroundColor: highlighted, outlineColor: 'var(--accent)', offset: .6 },
    { backgroundColor: background, outlineColor: 'transparent', offset: 1 },
  ], { duration: motionDuration('emphasis'), easing: 'linear' }) : null
  const timer = window.setTimeout(() => cleanup(), motionDuration('emphasis'))
  const cleanup = () => {
    clearTimeout(timer)
    animation?.cancel()
    element.classList.remove('motion-target')
    if (emphasis.get(element) === cleanup) emphasis.delete(element)
  }
  emphasis.set(element, cleanup)
  return cleanup
}

let cancelNavigation: (() => void) | undefined

export function scrollContainer(element: HTMLElement): HTMLElement {
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(parent).overflowY)) return parent
  }
  return document.documentElement
}

/** 只移动所属滚动容器。长距离直接定位，短距离可中断，避免浏览器滚动整个工作区。 */
export function revealAndEmphasize(target: HTMLElement, scroller: HTMLElement): () => void {
  cancelNavigation?.()
  let frame = 0
  let cancelled = false
  let cancelEmphasis: (() => void) | undefined
  const cancel = () => {
    cancelled = true
    cancelAnimationFrame(frame)
    cancelEmphasis?.()
    for (const type of ['wheel', 'pointerdown', 'keydown']) scroller.removeEventListener(type, cancel)
    if (cancelNavigation === cancel) cancelNavigation = undefined
  }
  cancelNavigation = cancel
  for (const type of ['wheel', 'pointerdown', 'keydown']) scroller.addEventListener(type, cancel, { passive: true })
  const reveal = () => {
    if (cancelled || !target.isConnected) return
    const bounds = scroller.getBoundingClientRect()
    const rect = target.getBoundingClientRect()
    const visible = rect.top >= bounds.top && rect.bottom <= bounds.bottom
    const from = scroller.scrollTop
    const to = Math.max(0, Math.min(scroller.scrollHeight - scroller.clientHeight,
      from + (rect.top + rect.bottom - bounds.top - bounds.bottom) / 2))
    const arrive = () => { if (target.isConnected) cancelEmphasis = emphasize(target) }
    if (visible) arrive()
    else if (reducedMotion() || Math.abs(to - from) > scroller.clientHeight) {
      scroller.scrollTop = to
      arrive()
    } else {
      const started = performance.now()
      const duration = motionDuration('layout')
      const tick = (now: number) => {
        const progress = Math.min(1, (now - started) / duration)
        // 短距离导航采用单调减速，用户输入会立即取消剩余帧。
        scroller.scrollTop = from + (to - from) * (1 - Math.pow(1 - progress, 3))
        if (progress < 1) frame = requestAnimationFrame(tick)
        else arrive()
      }
      frame = requestAnimationFrame(tick)
    }
  }
  // 诊断面板仍在展开时先等待布局落定，目标强调不被面板移动消耗。
  const layoutAnimations: Animation[] = []
  for (let parent: HTMLElement | null = scroller; parent; parent = parent.parentElement) {
    layoutAnimations.push(...parent.getAnimations().filter(animation =>
      animation.playState === 'running' && Number.isFinite(Number(animation.effect?.getComputedTiming().endTime))))
  }
  if (layoutAnimations.length) void Promise.allSettled(layoutAnimations.map(animation => animation.finished)).then(reveal)
  else reveal()
  return cancel
}
