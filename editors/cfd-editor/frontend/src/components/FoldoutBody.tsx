import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { motionDuration, motionToken, reducedMotion } from '../utils/motion'

/** 收回完成后才卸载子字段；反向操作从当前帧接续，不重复创建编辑控件。 */
export function FoldoutBody({ expanded, immediate, children }: { expanded: boolean; immediate: boolean; children: ReactNode }) {
  const [retained, setRetained] = useState(expanded)
  const ref = useRef<HTMLDivElement>(null)
  const animation = useRef<Animation | null>(null)
  const previous = useRef(expanded)
  useLayoutEffect(() => {
    const element = ref.current
    const changed = previous.current !== expanded
    previous.current = expanded
    if (!element) return
    const height = element.getBoundingClientRect().height
    const opacity = animation.current ? getComputedStyle(element).opacity : expanded ? '.65' : '1'
    animation.current?.cancel()
    animation.current = null
    element.style.removeProperty('height')
    element.style.removeProperty('overflow')
    element.style.removeProperty('opacity')
    if (!expanded && element.contains(document.activeElement)) {
      (element.previousElementSibling as HTMLElement | null)?.focus({ preventScroll: true })
    }
    element.inert = !expanded
    if (expanded) setRetained(true)
    if (!changed || immediate || reducedMotion()) {
      if (!expanded) setRetained(false)
      return
    }
    const motion = element.animate([
      { height: `${expanded && !retained ? 0 : height}px`, opacity, overflow: 'clip' },
      { height: `${expanded ? element.scrollHeight : 0}px`, opacity: expanded ? 1 : 0, overflow: 'clip' },
    ], { duration: motionDuration('layout'), easing: motionToken('layout-ease'), fill: 'both' })
    animation.current = motion
    motion.onfinish = () => {
      if (animation.current !== motion) return
      // 先锁住结束帧，再取消动画；React 卸载前也不会短暂恢复完整高度。
      if (!expanded) {
        element.style.height = '0px'
        element.style.overflow = 'clip'
        element.style.opacity = '0'
        setRetained(false)
      }
      motion.cancel()
      animation.current = null
    }
  }, [expanded, immediate])
  useLayoutEffect(() => {
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)')
    const finish = () => animation.current?.finish()
    preference.addEventListener('change', finish)
    window.addEventListener('resize', finish)
    return () => {
      animation.current?.cancel()
      preference.removeEventListener('change', finish)
      window.removeEventListener('resize', finish)
    }
  }, [])
  if (immediate ? !expanded : !expanded && !retained) return null
  return <div ref={ref} className="dc-group-body" aria-hidden={!expanded || undefined}>{children}</div>
}
