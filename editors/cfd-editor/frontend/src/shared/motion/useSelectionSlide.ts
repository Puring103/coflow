import { useLayoutEffect, useRef, type RefObject } from 'react'
import { motionDuration, motionToken, reducedMotion } from './motion'

/** 仅移动选中底色；正文、焦点和命中区域始终留在真实位置。 */
export function useSelectionSlide(root: RefObject<HTMLElement | null>, selector: string) {
  const previous = useRef<HTMLElement | null>(null)
  const motion = useRef<Animation | null>(null)
  useLayoutEffect(() => {
    const next = root.current?.querySelector<HTMLElement>(selector) ?? null
    const old = previous.current
    if (next === old) return
    const from = old?.isConnected ? old.getBoundingClientRect() : null
    const transform = old ? getComputedStyle(old, '::before').transform : 'none'
    const matrix = new DOMMatrix(transform === 'none' ? undefined : transform)
    motion.current?.cancel()
    if (old) { delete old.dataset.motionSelection; old.style.removeProperty('--selection-offset') }
    previous.current = next
    if (!next) return
    next.dataset.motionSelection = ''
    if (!from || reducedMotion()) return
    const to = next.getBoundingClientRect()
    if (!to.width || !to.height) return
    const dx = from.x + matrix.e - to.x
    const dy = from.y + matrix.f - to.y
    // 只衔接相邻选择；长列表跳转直接显示目标，底色不扫过大量无关条目。
    const viewport = root.current!.getBoundingClientRect()
    const horizontalLimit = Math.min(viewport.width, Math.max(240, Math.max(from.width, to.width) * 3))
    const verticalLimit = Math.min(viewport.height, Math.max(240, Math.max(from.height, to.height) * 3))
    if (Math.abs(dx) > horizontalLimit || Math.abs(dy) > verticalLimit) return
    // 首帧先落到旧位置再启动，避免伪元素首次生成时短暂绘制在终点。
    const offset = `translate(${dx}px, ${dy}px)`
    next.style.setProperty('--selection-offset', offset)
    void getComputedStyle(next, '::before').transform
    motion.current = next.animate([
      { transform: offset },
      { transform: 'none' },
    ], { pseudoElement: '::before', duration: motionDuration('layout'), easing: motionToken('layout-ease'), fill: 'both' })
    const animation = motion.current
    animation.onfinish = () => {
      if (motion.current !== animation) return
      next.style.removeProperty('--selection-offset')
      animation.cancel()
    }
  })
  useLayoutEffect(() => {
    const preference = matchMedia('(prefers-reduced-motion: reduce)')
    const finish = () => motion.current?.finish()
    preference.addEventListener('change', finish)
    window.addEventListener('resize', finish)
    return () => {
      motion.current?.cancel()
      preference.removeEventListener('change', finish)
      window.removeEventListener('resize', finish)
    }
  }, [])
}
