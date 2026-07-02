import type { MouseEvent as ReactMouseEvent } from 'react'

/** Corner marker that lights up on any record / field cell whose diagnostics
 *  are non-empty. Clicking it should scroll the diagnostics panel to the
 *  first matching item. The wrapping element must have `position:relative`. */
export function DiagBadge({
  severity,
  onClick,
  title,
}: {
  severity: 'error' | 'warning'
  onClick?: () => void
  title?: string
}) {
  return (
    <button
      type="button"
      className={`diag-corner-badge diag-corner-${severity}`}
      title={title ?? (severity === 'error' ? '查看错误诊断' : '查看警告诊断')}
      aria-label={severity === 'error' ? '存在错误，点击查看' : '存在警告，点击查看'}
      onClick={onClick ? (e: ReactMouseEvent) => {
        e.preventDefault()
        e.stopPropagation()
        onClick()
      } : undefined}
    >
      {severity === 'error' ? '!' : '?'}
    </button>
  )
}
