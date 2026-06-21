import { useState } from 'react'
import type { DiagnosticItem } from '../bindings/index'
import { Icon } from './Icon'

interface Props {
  diagnostics: DiagnosticItem[]
  onJumpToRecord?: (file: string, key: string) => void
}

export function DiagnosticsPanel({ diagnostics, onJumpToRecord }: Props) {
  const [collapsed, setCollapsed] = useState(false)

  const errors   = diagnostics.filter(d => d.severity === 'error').length
  const warnings = diagnostics.filter(d => d.severity === 'warning').length

  // No diagnostics → keep header only, no body region.
  const isEmpty = diagnostics.length === 0
  return (
    <div className={`diag-panel${collapsed || isEmpty ? ' collapsed' : ''}`}>
      <div className="diag-header" onClick={() => setCollapsed(c => !c)}>
        <span className="diag-title">
          <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={11} />
          诊断
        </span>
        {errors > 0 && (
          <span className="diag-badge error">
            <Icon name="error" size={11} />
            {errors}
          </span>
        )}
        {warnings > 0 && (
          <span className="diag-badge warning">
            <Icon name="warning" size={11} />
            {warnings}
          </span>
        )}
        {errors === 0 && warnings === 0 && (
          <span className="diag-badge ok">
            <Icon name="check" size={11} />
            无问题
          </span>
        )}
      </div>
      {!collapsed && !isEmpty && (
        <div className="diag-list">
          {diagnostics.map((d, i) => (
            <div key={i} className={`diag-item ${d.severity}`}>
              <span className="diag-icon">
                <Icon
                  name={d.severity === 'error' ? 'error' : d.severity === 'warning' ? 'warning' : 'info'}
                  size={14}
                />
              </span>
              <span className="diag-msg">{d.message}</span>
              {d.code && <span className="diag-code">{d.code}</span>}
              {d.file_path && d.record_key && onJumpToRecord && (
                <button
                  className="diag-jump"
                  onClick={() => onJumpToRecord(d.file_path!, d.record_key!)}
                >
                  <Icon name="jump" size={11} />
                  跳转
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
