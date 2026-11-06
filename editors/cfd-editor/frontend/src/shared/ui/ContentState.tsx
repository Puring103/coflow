import type { ReactNode } from 'react'
import { Icon } from './Icon'
import './ContentState.css'

interface Props {
  kind: 'loading' | 'empty' | 'error'
  title: string
  description?: string
  compact?: boolean
  children?: ReactNode
}

/** 只呈现调用方已有的状态，不参与加载、重试或内容切换时机。 */
export function ContentState({ kind, title, description, compact = false, children }: Props) {
  return (
    <div
      className={`content-state content-state-${kind}${compact ? ' content-state-compact' : ''}`}
      role={kind === 'error' ? 'alert' : kind === 'loading' ? 'status' : undefined}
    >
      <span className="content-state-icon" aria-hidden="true">
        <Icon name={kind === 'loading' ? 'refresh' : kind === 'error' ? 'error' : 'info'} size={compact ? 20 : 28} className={kind === 'loading' ? 'icon-spin' : undefined} />
      </span>
      <div className="content-state-copy">
        <div className="content-state-title">{title}</div>
        {description && <div className="content-state-description">{description}</div>}
      </div>
      {children && <div className="content-state-actions">{children}</div>}
    </div>
  )
}
