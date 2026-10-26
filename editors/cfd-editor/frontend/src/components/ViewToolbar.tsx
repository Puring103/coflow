import { useEffect, useRef, type KeyboardEventHandler, type ReactNode, type Ref } from 'react'
import { Icon } from './Icon'

/** 主视图和业务延伸视图共用工具栏尺寸、搜索控件和键盘入口。 */
export function ViewToolbar({ children, className = '' }: { children: ReactNode; className?: string }) {
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const focusSearch = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.shiftKey || event.key.toLowerCase() !== 'f') return
      const input = root.current?.querySelector<HTMLInputElement>('[role="searchbox"]')
      if (!input || input.getClientRects().length === 0) return
      event.preventDefault()
      input.focus()
    }
    window.addEventListener('keydown', focusSearch)
    return () => window.removeEventListener('keydown', focusSearch)
  }, [])
  return <div ref={root} className={`global-search-bar ${className}`}>{children}</div>
}

export function ViewSearchField({ value, onChange, label, placeholder, inputRef, onKeyDown, children }: {
  value: string
  onChange: (value: string) => void
  label: string
  placeholder: string
  inputRef?: Ref<HTMLInputElement>
  onKeyDown?: KeyboardEventHandler<HTMLInputElement>
  children?: ReactNode
}) {
  return <div className="record-search">
    <Icon name="search" size={13} className="global-search-icon" aria-hidden />
    <input ref={inputRef} role="searchbox" aria-label={label} placeholder={placeholder}
      value={value} onChange={event => onChange(event.target.value)} onKeyDown={event => {
        if (event.key === 'Escape') { event.preventDefault(); onChange('') }
        onKeyDown?.(event)
      }} />
    {value && <button className="rv-clear-search" aria-label="清除搜索" onClick={() => onChange('')}><Icon name="close" size={13} /></button>}
    {children}
  </div>
}
