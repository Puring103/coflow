import { revealTableCell } from '../lib/tableViewport'
import { useLayoutEffect, useRef, type RefObject } from 'react'
import { motionDuration, motionToken, reducedMotion } from './motion'

/** 选框独立于正文移动；原位预览仅裁切揭示，不缩放文字或改变表格行高。 */
export function useCellMotion(root: RefObject<HTMLDivElement | null>) {
  const previous = useRef<HTMLElement | null>(null)
  const cleanup = useRef<(() => void) | null>(null)
  const frame = useRef<HTMLDivElement | null>(null)
  useLayoutEffect(() => {
    const cells = root.current?.querySelectorAll<HTMLElement>('td.selected-cell')
    const next = cells?.length === 1 ? cells[0] : null
    if (previous.current === next) return
    const old = previous.current
    const from = frame.current?.getBoundingClientRect() ?? (old?.isConnected ? old.getBoundingClientRect() : null)
    cleanup.current?.()
    previous.current = next
    if (!next || !root.current) return
    const scroller = root.current
    // 使用冻结列右缘与表头下缘作为有效视口，不能只检查滚动容器外框。
    const ensureVisible = (bounds: DOMRect) => revealTableCell(scroller, next, bounds)
    ensureVisible(next.getBoundingClientRect())
    const cell = next.getBoundingClientRect()
    const animations: Animation[] = []
    let revealed = false
    let raf = 0
    let overlay: HTMLDivElement | null = null
    let moving: Animation | null = null
    let beginExpansion: (() => void) | null = null
    let expandedPreview: HTMLElement | null = null
    const releaseFrame = () => {
      delete next.dataset.cellMotion
      overlay?.remove()
      if (frame.current === overlay) frame.current = null
    }
    const reveal = () => {
      if (revealed || !next.isConnected) return
      const preview = next.querySelector<HTMLElement>('.cell-complex-preview.overflowing .cell-complex-content, .cell-string-input:not(.focused) .cell-string-preview')
      const bounds = preview?.getBoundingClientRect()
      const currentCell = next.getBoundingClientRect()
      if (!preview || !bounds || bounds.height <= currentCell.height + 1) return
      revealed = true
      ensureVisible(bounds)
      if (reducedMotion()) return
      const finalBounds = preview.getBoundingClientRect()
      const collapsedHeight = Math.min(finalBounds.height, next.getBoundingClientRect().bottom - finalBounds.top)
      const clipped = `inset(-24px -24px ${finalBounds.height - collapsedHeight}px -24px)`
      // 平移阶段保持折叠边界；到达后，内容裁切与选框高度共用同一时钟和曲线。
      expandedPreview = preview
      preview.style.clipPath = clipped
      beginExpansion = () => {
        beginExpansion = null
        if (!overlay?.isConnected) {
          overlay = document.createElement('div')
          overlay.className = 'cell-selection-motion'
          overlay.setAttribute('aria-hidden', 'true')
          document.body.append(overlay)
        }
        frame.current = overlay
        next.dataset.cellMotion = ''
        Object.assign(overlay.style, {
          left: `${finalBounds.x}px`, top: `${finalBounds.y}px`, width: `${finalBounds.width}px`,
          height: `${collapsedHeight}px`, borderWidth: '0', outline: '1px solid var(--accent)',
        })
        const options: KeyframeAnimationOptions = {
          duration: motionDuration('expand'), easing: motionToken('enter-ease'), fill: 'both',
        }
        const contentAnimation = preview.animate([{ clipPath: clipped }, { clipPath: 'inset(-24px -24px 0px -24px)' }], options)
        const borderAnimation = overlay.animate([{ height: `${collapsedHeight}px` }, { height: `${finalBounds.height}px` }], options)
        const start = document.timeline.currentTime
        contentAnimation.startTime = start
        borderAnimation.startTime = start
        borderAnimation.onfinish = () => {
          if (frame.current !== overlay) return
          preview.style.removeProperty('clip-path')
          contentAnimation.cancel()
          releaseFrame()
          borderAnimation.cancel()
        }
        animations.push(contentAnimation, borderAnimation)
      }
      if (!moving || moving.playState === 'finished') {
        moving?.cancel()
        moving = null
        beginExpansion()
      }

    }
    // 溢出状态由子组件测量后更新；监听真实内容，避免首次选择早于测量而漏播。
    const observer = new MutationObserver(() => { cancelAnimationFrame(raf); raf = requestAnimationFrame(reveal) })
    observer.observe(next, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] })
    raf = requestAnimationFrame(reveal)
    const to = cell
    if (from && !reducedMotion()) {
      overlay = document.createElement('div')
      overlay.className = 'cell-selection-motion'
      overlay.setAttribute('aria-hidden', 'true')
      Object.assign(overlay.style, { left: `${to.x}px`, top: `${to.y}px`, width: `${to.width}px`, height: `${to.height}px` })
      // 覆盖层挂载前固定首帧，防止先画到终点再跳回起点。
      overlay.style.left = `${from.x}px`
      overlay.style.top = `${from.y}px`
      document.body.append(overlay)
      frame.current = overlay
      next.dataset.cellMotion = ''
      const animation = overlay.animate([
        { left: `${from.x}px`, top: `${from.y}px` },
        { left: `${to.x}px`, top: `${to.y}px` },
      ], { duration: motionDuration('feedback'), easing: motionToken('enter-ease'), fill: 'both' })
      moving = animation
      animation.onfinish = () => {
        if (frame.current !== overlay || moving !== animation) return
        moving = null
        if (beginExpansion) { animation.cancel(); beginExpansion() }
        else releaseFrame()
      }
      animations.push(animation)
    }
    const preference = matchMedia('(prefers-reduced-motion: reduce)')
    const stop = () => {
      observer.disconnect()
      cancelAnimationFrame(raf)
      animations.forEach(animation => animation.cancel())
      expandedPreview?.style.removeProperty('clip-path')
      overlay?.remove()
      delete next.dataset.cellMotion
      frame.current = null
      scroller.removeEventListener('scroll', stop)
      window.removeEventListener('wheel', stop, true)
      window.removeEventListener('resize', stop)
      preference.removeEventListener('change', stop)
    }
    scroller.addEventListener('scroll', stop, { passive: true })
    window.addEventListener('wheel', stop, true)
    window.addEventListener('resize', stop)
    preference.addEventListener('change', stop)
    cleanup.current = stop
  })
  useLayoutEffect(() => () => cleanup.current?.(), [])
}
