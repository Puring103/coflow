import { useSurfaceExit } from '../hooks/useSurfaceExit'
import { useEffect, useRef, useState } from 'react'
import { Icon } from './Icon'

interface BaseProps {
  title: string
  message: string
  confirmLabel: string
  danger?: boolean
  busy?: boolean
  onConfirm: (value: string) => void | Promise<void>
  onClose: () => void
}

export function TextInputDialog({ initialValue = '', placeholder, suffix, ...props }: BaseProps & {
  initialValue?: string
  placeholder?: string
  suffix?: string
}) {
  const [value, setValue] = useState(initialValue)
  return <ActionDialog {...props} value={value.trim()} input={(
    <div className="action-dialog-input-wrap">
      <input
        ref={element => element?.focus()}
        className="action-dialog-input"
        value={value}
        placeholder={placeholder}
        onChange={event => setValue(event.target.value)}
      />
      {suffix && <span className="action-dialog-input-suffix">{suffix}</span>}
    </div>
  )} />
}

export function ConfirmDialog(props: BaseProps) {
  return <ActionDialog {...props} value="" />
}

function ActionDialog({ title, message, confirmLabel, danger, busy, onConfirm, onClose, value, input }: BaseProps & {
  value: string
  input?: React.ReactNode
}) {
  const dialogRef = useRef<HTMLElement>(null)
  const exit = useSurfaceExit(dialogRef)
  const dismiss = () => { if (!busy) void exit(onClose) }
  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>('input, select, button')?.focus()
  }, [])
  return (
    <div className="create-record-backdrop" role="presentation" onMouseDown={event => {
      if (event.target === event.currentTarget && !busy) dismiss()
    }}>
      <section
        ref={dialogRef}
        className="create-record-dialog action-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onKeyDown={event => {
          if (event.key === 'Escape' && !busy) dismiss()
          if (event.key === 'Enter' && (!input || value) && !busy) void onConfirm(value)
        }}
      >
        <header className="action-dialog-header">
          <strong>{title}</strong>
          <button className="btn-icon" onClick={dismiss} disabled={busy} aria-label={`关闭${title}`}>
            <Icon name="close" size={14} />
          </button>
        </header>
        <div className="action-dialog-body">
          <p>{message}</p>
          {input}
        </div>
        <footer className="create-record-actions">
          <button className="btn" onClick={dismiss} disabled={busy}>取消</button>
          <button className={`btn ${danger ? 'btn-danger' : 'btn-primary'}`} onClick={() => void onConfirm(value)} disabled={busy || (!!input && !value)}>
            {busy ? '处理中...' : confirmLabel}
          </button>
        </footer>
      </section>
    </div>
  )
}
