import { useEffect, useRef, type ReactNode } from 'react'

/** 轻量菜单统一处理外部点击和 Escape，关闭后保留稳定的触发入口。 */
export function Menu({ className, label, title, children }: {
  className: string
  label: ReactNode
  title?: string
  children: ReactNode
}) {
  const ref = useRef<HTMLDetailsElement>(null)
  useEffect(() => {
    const close = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) ref.current.open = false
    }
    document.addEventListener('pointerdown', close)
    return () => document.removeEventListener('pointerdown', close)
  }, [])
  return <details ref={ref} className={className} onKeyDown={event => {
    if (event.key === 'Escape' && event.currentTarget.open) {
      event.stopPropagation()
      event.currentTarget.open = false
      event.currentTarget.querySelector('summary')?.focus()
    }
  }} onBlur={event => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) event.currentTarget.open = false
  }}>
    <summary title={title} aria-label={title}>{label}</summary>
    <div onClick={event => {
      if ((event.target as HTMLElement).closest('button:not(:disabled)') && ref.current) ref.current.open = false
    }}>{children}</div>
  </details>
}
