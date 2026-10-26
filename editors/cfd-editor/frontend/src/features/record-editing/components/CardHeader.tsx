import { useState, useEffect, useContext, type CSSProperties } from 'react'
import type { FieldCell } from '../../../bindings/FieldCell'

import { DiagBadge } from '../../diagnostics/components/DiagBadge'
import { typeColor } from '../../../shared/lib/typeColor'

import { ShortNameContext } from './ShortNameContext'
import { recordShortName, shortNameLabel } from '../state/shortNames'

// 记录标题与重命名交互。
export function CardHeader({
  recordId,
  shortName,
  fields,
  actualType,
  filePath,
  onRename,
  diagSeverity,
  onDiagBadgeClick,
}: {
  recordId: string
  shortName?: string
  fields?: FieldCell[]
  actualType: string
  filePath?: string
  onRename?: (newId: string) => void
  /** Record-level severity: shows a corner badge that focuses the panel. */
  diagSeverity?: 'error' | 'warning' | null
  onDiagBadgeClick?: () => void
  /** When true, the header briefly pulses (record-level diagnostic jump). */
}) {
  const color = typeColor(actualType)
  const shortNameSettings = useContext(ShortNameContext)
  const displayName = shortName ?? (fields ? recordShortName(fields, shortNameSettings.fields[actualType]) : undefined)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(recordId)
  useEffect(() => { if (!editing) setDraft(recordId) }, [recordId, editing])

  const commit = () => {
    const next = draft.trim()
    setEditing(false)
    if (next && next !== recordId && onRename) onRename(next)
  }

  return (
    <div className="gn-header" style={{ '--node-color': color } as CSSProperties}>
      <div className="gn-color-bar" />
      {editing ? (
        <input
          className="gn-key-editor"
          value={draft}
          autoFocus
          onChange={e => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={e => {
            if (e.key === 'Enter') commit()
            if (e.key === 'Escape') { setEditing(false); setDraft(recordId) }
          }}
          onClick={e => e.stopPropagation()}
        />
      ) : (
        <span
          className={`gn-key${onRename ? ' gn-key-renameable' : ''}`}
          onDoubleClick={onRename ? () => setEditing(true) : undefined}
          title={onRename ? '双击重命名' : undefined}
        >
          {shortNameLabel(recordId, displayName)}
        </span>
      )}
      <div className="gn-meta">
        <span className="gn-type">{actualType}</span>
        {filePath && <span className="gn-file">{filePath.split('/').pop()}</span>}
      </div>
      {(diagSeverity === 'error' || diagSeverity === 'warning') && (
        <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
      )}
    </div>
  )
}
