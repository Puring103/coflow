import { useEffect, useRef, type RefObject } from 'react'
import { motionDuration, motionToken, reducedMotion } from './motion'

/** 普通窗口退出只负责视觉收尾；提交、校验和导航仍由调用方先完成。 */
export function useSurfaceExit(ref: RefObject<HTMLElement | null>, identity: boolean | string = true) {
  const exit = useRef<(() => void) | null>(null)
  const generation = useRef(0)
  useEffect(() => {
    generation.current++
    return () => { generation.current++; exit.current?.(); exit.current = null }
  }, [identity])
  return async (action: () => void, options?: { transform?: string; fadeOverlay?: boolean }) => {
    if (exit.current) return
    const current = generation.current
    const element = ref.current
    if (!element || reducedMotion()) { action(); return }
    const visual = getComputedStyle(element)
    const start = { opacity: visual.opacity, transform: visual.transform }
    element.getAnimations().forEach(animation => animation.cancel())
    const previousInert = element.inert
    element.inert = true
    element.dataset.motionExiting = 'true'
    const overlay = element.closest<HTMLElement>('.create-record-backdrop, .help-overlay, .reference-panel-overlay')
    let fade: Animation | undefined
    if (overlay && options?.fadeOverlay !== false) {
      const opacity = getComputedStyle(overlay).opacity
      overlay.getAnimations().forEach(animation => animation.cancel())
      fade = overlay.animate([{ opacity }, { opacity: 0 }], { duration: motionDuration('exit'), fill: 'forwards' })
    }
    const animation = element.animate([start, { opacity: 0, transform: options?.transform ?? 'translateY(4px)' }], {
      duration: motionDuration('exit'), easing: motionToken('exit-ease'), fill: 'forwards',
    })
    exit.current = () => {
      animation.cancel()
      fade?.cancel()
      element.inert = previousInert
      delete element.dataset.motionExiting
    }
    await animation.finished.catch(() => {})
    if (generation.current !== current) return
    action()
    // 结束帧保留到卸载；避免退出完成后出现一帧完整窗口。
  }
}
