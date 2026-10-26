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
    // 首帧先落到旧位置再启动，避免伪元素首次生成时短暂绘制在终点。
    const offset = `translate(${from.x + matrix.e - to.x}px, ${from.y + matrix.f - to.y}px)`
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
