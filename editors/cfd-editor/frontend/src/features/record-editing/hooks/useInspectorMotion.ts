import { useLayoutEffect, useRef, useState } from 'react'
import { flushDataEdits } from '../state/editSession'
import { emphasize, motionDuration, motionToken, originBounds, reducedMotion, type MotionOrigin } from '../../../shared/motion/motion'

export function useInspectorMotion(open: boolean, modal: boolean, origin: MotionOrigin | null | undefined, onClose: () => void) {
  const panelRef = useRef<HTMLElement>(null)
  const shellRef = useRef<HTMLDivElement>(null)
  const backdropRef = useRef<HTMLDivElement>(null)
  const session = useRef<{ close: () => Promise<void> } | null>(null)
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useLayoutEffect(() => {
    if (!modal || !open) return
    const panel = panelRef.current!
    const shell = shellRef.current!
    const backdrop = backdropRef.current!
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const app = document.querySelector<HTMLElement>('.app')
    const wasInert = app?.inert ?? false
    const previousOverflow = document.body.style.overflow
    let active = true
    let closing = false
    let timer = 0
    let animations: Animation[] = []
    setBusy(false)
    setNotice(null)
    if (origin) origin.changed = false
    if (app) app.inert = true
    document.body.style.overflow = 'hidden'
    panel.focus({ preventScroll: true })

    const resetGeometry = () => {
      shell.style.removeProperty('width')
      shell.style.removeProperty('height')
      shell.style.removeProperty('transform')
      shell.style.removeProperty('overflow')
      panel.style.removeProperty('width')
      panel.style.removeProperty('height')
    }
    const cancelAnimations = () => { animations.forEach(animation => animation.cancel()); animations = [] }
    let target = shell.getBoundingClientRect()
    const freezeContent = () => {
      // 正文始终按最终尺寸排版，只有外壳移动和裁切，不缩放字号或反复换行。
      const style = getComputedStyle(shell)
      panel.style.width = `${target.width - parseFloat(style.borderLeftWidth) - parseFloat(style.borderRightWidth)}px`
      panel.style.height = `${target.height - parseFloat(style.borderTopWidth) - parseFloat(style.borderBottomWidth)}px`
      shell.style.overflow = 'clip'
    }
    const originFrame = (rect: DOMRect) => ({
      transform: `translate(${rect.x - target.x}px, ${rect.y - target.y}px)`,
      width: `${rect.width}px`, height: `${rect.height}px`, borderRadius: '2px',
      boxShadow: '0 2px 6px #0002',
    })
    const surfaceColor = getComputedStyle(backdrop).backgroundColor
    const shadow = getComputedStyle(shell).boxShadow
    const radius = getComputedStyle(shell).borderRadius
    const rect = originBounds(origin)
    if (!reducedMotion()) {
      freezeContent()
      const duration = motionDuration(rect ? 'expand' : 'enter')
      const surface = shell.animate([
        rect ? originFrame(rect) : { transform: 'translateY(4px)', opacity: 0 },
        { transform: 'none', width: `${target.width}px`, height: `${target.height}px`, borderRadius: radius, opacity: 1, boxShadow: shadow },
      ], { duration, easing: motionToken('enter-ease'), fill: 'both' })
      animations = [surface,
        panel.animate([{ opacity: 0 }, { opacity: 1 }], {
          duration: motionDuration('enter'), fill: 'both', easing: 'linear',
        }),
        backdrop.animate([{ backgroundColor: 'transparent' }, { backgroundColor: surfaceColor }], {
          duration: motionDuration('enter'), fill: 'both', easing: 'linear',
        }),
      ]
      void surface.finished.then(() => {
        // 等待保存期间也释放已完成的入场尺寸；退场接管后只处理自己的动画。
        if (!active || !animations.includes(surface)) return
        cancelAnimations()
        resetGeometry()
      }).catch(() => {})
    }

    const close = async () => {
      if (!active || closing) return
      closing = true
      setBusy(true)
      const editFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
      timer = window.setTimeout(() => { if (active) setNotice('正在保存…') }, 200)
      try {
        const pending = flushDataEdits()
        // 提交期间冻结本次编辑会话；失败后恢复输入与草稿，不启动退场。
        panel.inert = true
        const saved = await pending
        if (!active) return
        clearTimeout(timer)
        if (!saved) {
          closing = false
          panel.inert = false
          setBusy(false)
          setNotice('保存失败，请检查字段错误后重试')
          if (editFocus?.isConnected) editFocus.focus({ preventScroll: true })
          return
        }
        setNotice(null)
        if (!reducedMotion()) {
          if (!animations.length) target = shell.getBoundingClientRect()
          const visual = getComputedStyle(shell)
          const start = { transform: visual.transform, width: visual.width, height: visual.height,
            borderRadius: visual.borderRadius, opacity: visual.opacity, boxShadow: visual.boxShadow }
          const contentOpacity = getComputedStyle(panel).opacity
          const backgroundColor = getComputedStyle(backdrop).backgroundColor
          cancelAnimations()
          freezeContent()
          // 重新解析业务来源；来源不可见时淡出，避免飞向旧坐标。
          const destination = originBounds(origin)
          const duration = motionDuration(origin && !destination ? 'feedback' : 'exit')
          const surface = shell.animate([start, destination
            ? { ...originFrame(destination), opacity: 0 }
            : { ...start, transform: origin ? start.transform : 'translateY(4px)', opacity: 0 }], { duration, easing: motionToken('exit-ease'), fill: 'forwards' })
          animations = [surface,
            panel.animate([{ opacity: contentOpacity }, { opacity: 0 }], { duration: Math.min(100, duration), fill: 'forwards' }),
            backdrop.animate([{ backgroundColor }, { backgroundColor: 'transparent' }], { duration, fill: 'forwards' }),
          ]
          await surface.finished.catch(() => {})
        }
        if (!active) return
        onCloseRef.current()
        if (app) app.inert = wasInert
        if (origin) {
          origin.focus()
          const source = origin.resolve()
          if (origin.changed && source && originBounds(origin)) emphasize(source)
        }
        else if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true })
      } catch (error) {
        if (!active) return
        closing = false
        panel.inert = false
        setBusy(false)
        setNotice(`保存失败：${error instanceof Error ? error.message : String(error)}`)
        if (editFocus?.isConnected) editFocus.focus({ preventScroll: true })
      } finally { clearTimeout(timer) }
    }
    session.current = { close }
    // 窗口变化或系统减少动态设置变化时结束当前动效，使用响应式最终布局。
    const settle = () => animations.forEach(animation => { if (animation.playState !== 'idle') animation.finish() })
    const preference = matchMedia('(prefers-reduced-motion: reduce)')
    const blockClosingKeys = (event: KeyboardEvent) => {
      if (closing) { event.preventDefault(); event.stopImmediatePropagation() }
    }
    window.addEventListener('resize', settle)
    window.addEventListener('keydown', blockClosingKeys, true)
    preference.addEventListener('change', settle)
    return () => {
      active = false
      clearTimeout(timer)
      cancelAnimations()
      resetGeometry()
      panel.inert = false
      session.current = null
      window.removeEventListener('resize', settle)
      window.removeEventListener('keydown', blockClosingKeys, true)
      preference.removeEventListener('change', settle)
      if (app) app.inert = wasInert
      document.body.style.overflow = previousOverflow
    }
  }, [open, modal, origin])

  return { panelRef, shellRef, backdropRef, notice, busy, close: () => session.current?.close() }
}
