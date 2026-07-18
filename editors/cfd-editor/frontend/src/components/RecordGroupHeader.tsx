import { useState } from 'react'
import { Icon } from './Icon'

interface Props {
  name: string
  groupId: string
  count: number
  collapsed: boolean
  className?: string
  onToggle: () => void
  onRename: (name: string) => void
}

export function RecordGroupHeader({ name, groupId, count, collapsed, className, onToggle, onRename }: Props) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(name)

  const commitRename = () => {
    const next = draft.trim()
    if (next && next !== name) onRename(next)
    else setDraft(name)
    setEditing(false)
  }

  return (
    <div
      className={`record-group-header${className ? ` ${className}` : ''}`}
      data-record-drop-kind="group"
      data-record-group-id={groupId}
    >
      <button
        type="button"
        className="record-group-toggle"
        onClick={onToggle}
        aria-expanded={!collapsed}
        title={collapsed ? '展开分组' : '折叠分组'}
      >
        <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={12} aria-hidden />
      </button>
      {editing ? (
        <input
          className="record-group-name-input"
          value={draft}
          maxLength={80}
          autoFocus
          aria-label="分组名称"
          onChange={event => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={event => {
            if (event.key === 'Enter') {
              event.preventDefault()
              commitRename()
            } else if (event.key === 'Escape') {
              event.preventDefault()
              setDraft(name)
              setEditing(false)
            }
          }}
        />
      ) : (
        <button type="button" className="record-group-name" onClick={onToggle} title={name}>
          {name}
        </button>
      )}
      <span className="record-group-count">{count}</span>
      <button
        type="button"
        className="record-group-rename"
        onClick={() => {
          setDraft(name)
          setEditing(true)
        }}
        aria-label={`重命名分组 ${name}`}
        title="重命名分组"
      >
        <Icon name="edit" size={12} aria-hidden />
      </button>
    </div>
  )
}

interface UngroupedProps {
  count: number
  className?: string
}

export function RecordUngroupedHeader({ count, className }: UngroupedProps) {
  return (
    <div
      className={`record-ungrouped-header${className ? ` ${className}` : ''}`}
      data-record-drop-kind="ungrouped"
    >
      <span className="record-group-label">未分组</span>
      <span className="record-group-count">{count}</span>
    </div>
  )
}
