import { useEffect, useState, type CSSProperties } from 'react'
import { Icon } from './Icon'

export const RECORD_GROUP_COLORS = [
  ['red', '#d9534f'],
  ['orange', '#e67e22'],
  ['yellow', '#d4a017'],
  ['green', '#3a9d5d'],
  ['cyan', '#2497a8'],
  ['blue', '#3978c6'],
  ['purple', '#8656b5'],
  ['gray', '#737b87'],
] as const

export function recordGroupColorStyle(color: string | null | undefined): CSSProperties | undefined {
  const entry = RECORD_GROUP_COLORS.find(([token]) => token === color)
  return entry ? { '--record-group-color': entry[1] } as CSSProperties : undefined
}

interface Props {
  name: string
  groupId: string
  count: number
  collapsed: boolean
  color?: string | null
  className?: string
  onToggle: () => void
  onRename: (name: string) => void
  onColorChange?: (color: string | null) => void
}

export function RecordGroupHeader({ name, groupId, count, collapsed, color, className, onToggle, onRename, onColorChange }: Props) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(name)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)

  useEffect(() => {
    if (!menu) return
    const close = (event: PointerEvent) => {
      if (event.target instanceof Element && event.target.closest('.record-group-color-menu')) return
      setMenu(null)
    }
    const closeOnBlur = () => setMenu(null)
    window.addEventListener('pointerdown', close)
    window.addEventListener('blur', closeOnBlur)
    return () => {
      window.removeEventListener('pointerdown', close)
      window.removeEventListener('blur', closeOnBlur)
    }
  }, [menu])

  const commitRename = () => {
    const next = draft.trim()
    if (next && next !== name) onRename(next)
    else setDraft(name)
    setEditing(false)
  }

  return (
    <div
      className={`record-group-header${color ? ' has-color' : ''}${className ? ` ${className}` : ''}`}
      data-record-drop-kind="group"
      data-record-group-id={groupId}
      style={recordGroupColorStyle(color)}
      role="button"
      tabIndex={0}
      aria-expanded={!collapsed}
      title={collapsed ? '展开分组' : '折叠分组'}
      onClick={() => { if (!editing) onToggle() }}
      onKeyDown={event => {
        if (editing || (event.key !== 'Enter' && event.key !== ' ')) return
        event.preventDefault()
        onToggle()
      }}
      onContextMenu={event => {
        event.preventDefault()
        event.stopPropagation()
        setMenu({ x: event.clientX, y: event.clientY })
      }}
    >
      <span className="record-group-toggle" aria-hidden>
        <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={12} />
      </span>
      {editing ? (
        <input
          className="record-group-name-input"
          value={draft}
          maxLength={80}
          autoFocus
          aria-label="分组名称"
          onClick={event => event.stopPropagation()}
          onChange={event => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={event => {
            event.stopPropagation()
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
        <span
          className="record-group-name"
          onDoubleClick={event => {
            event.stopPropagation()
            setDraft(name)
            setEditing(true)
          }}
        >
          {name}
        </span>
      )}
      <span className="record-group-count">{count}</span>
      {menu && onColorChange && (
        <div
          className="context-menu record-group-color-menu"
          style={{ left: menu.x, top: menu.y }}
          role="menu"
          onPointerDown={event => event.stopPropagation()}
          onClick={event => event.stopPropagation()}
        >
          <button
            type="button"
            className={`ctx-item${color ? '' : ' active'}`}
            role="menuitem"
            onClick={() => { onColorChange(null); setMenu(null) }}
          >
            <Icon name="close" size={13} aria-hidden />
            无颜色
          </button>
          <div className="record-group-color-grid" aria-label="常用颜色">
            {RECORD_GROUP_COLORS.map(([token, value]) => (
              <button
                key={token}
                type="button"
                className={`record-group-color-swatch${color === token ? ' active' : ''}`}
                style={{ background: value }}
                aria-label={token}
                title={token}
                onClick={() => { onColorChange(token); setMenu(null) }}
              />
            ))}
          </div>
        </div>
      )}
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
