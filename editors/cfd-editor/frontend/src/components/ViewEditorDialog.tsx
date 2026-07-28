import { useMemo, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { ViewConfig } from '../bindings/ViewConfig'
import type { ViewKind } from '../bindings/ViewKind'
import { newViewId } from '../state/views'
import { Icon } from './Icon'

interface Props {
  /** null => create mode; an existing ViewConfig => edit mode. */
  initial: ViewConfig | null
  /** All top-level field names selectable as columns / graph card fields. */
  availableFields: string[]
  /** All relation field-paths selectable for graph views. */
  availableRelations: string[]
  /** Record groups available for the group filter. */
  groups: readonly EditorRecordGroup[]
  onSubmit: (view: ViewConfig) => void
  onClose: () => void
}

/** Toggle membership while preserving selection order (append on add). */
function toggle(list: string[], value: string): string[] {
  return list.includes(value) ? list.filter(v => v !== value) : [...list, value]
}

export function ViewEditorDialog({
  initial,
  availableFields,
  availableRelations,
  groups,
  onSubmit,
  onClose,
}: Props) {
  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<ViewKind>(initial?.kind ?? 'table')
  // Which fields are checked (membership only; ordering is separate).
  const [selected, setSelected] = useState<Set<string>>(() => new Set(
    initial ? (initial.kind === 'table' ? initial.columns : initial.fields) : [],
  ))
  // The full field list in display order. Drag reorders this in place; a
  // field's row never moves just because it was (un)checked. Seeded so any
  // saved order comes first, then remaining fields in schema order.
  const [order, setOrder] = useState<string[]>(() => {
    const saved = initial ? (initial.kind === 'table' ? initial.columns : initial.fields) : []
    const rest = availableFields.filter(f => !saved.includes(f))
    return [...saved.filter(f => availableFields.includes(f)), ...rest]
  })
  const [relations, setRelations] = useState<string[]>(initial?.relations ?? [])
  const [groupFilter, setGroupFilter] = useState<string | null>(initial?.group_filter ?? null)
  // Pointer-based drag (native HTML5 drag is unreliable in the Tauri webview,
  // matching the record list's approach). `dragField` is the row being moved.
  const [dragField, setDragField] = useState<string | null>(null)
  const listRef = useRef<HTMLUListElement | null>(null)
  const dragStateRef = useRef<{ field: string; startY: number; moved: boolean } | null>(null)

  const trimmedName = name.trim()
  const relationSet = useMemo(() => new Set(relations), [relations])
  // Graph views: fields exclude already-selected relations (a relation renders
  // as an edge, not a card field). Preserves the persistent drag order.
  const fieldRows = useMemo(
    () => order.filter(f => availableFields.includes(f) && !(kind === 'graph' && relationSet.has(f))),
    [order, availableFields, kind, relationSet],
  )

  function submit() {
    if (!trimmedName) return
    // Output only checked fields, in display order.
    const cleanFields = fieldRows.filter(f => selected.has(f))
    onSubmit({
      id: initial?.id ?? newViewId(),
      name: trimmedName,
      kind,
      group_filter: groupFilter,
      columns: kind === 'table' ? cleanFields : [],
      column_widths: kind === 'table' ? (initial?.column_widths ?? {}) : {},
      relations: kind === 'graph' ? relations : [],
      fields: kind === 'graph' ? cleanFields : [],
    })
    onClose()
  }

  function toggleSelected(field: string) {
    setSelected(cur => {
      const next = new Set(cur)
      if (next.has(field)) next.delete(field); else next.add(field)
      return next
    })
  }

  // Move `dragged` to sit immediately before `targetField` in the full order.
  function reorder(dragged: string, targetField: string) {
    if (dragged === targetField) return
    setOrder(cur => {
      const without = cur.filter(f => f !== dragged)
      const to = without.indexOf(targetField)
      if (to < 0) return cur
      const next = without.slice()
      next.splice(to, 0, dragged)
      return next
    })
  }

  // The field name of the row under a given viewport Y coordinate, if any.
  function fieldUnderPointer(clientY: number): string | null {
    const rows = listRef.current?.querySelectorAll<HTMLElement>('[data-field]')
    if (!rows) return null
    for (const row of rows) {
      const rect = row.getBoundingClientRect()
      if (clientY >= rect.top && clientY <= rect.bottom) return row.dataset.field ?? null
    }
    return null
  }

  function onRowPointerDown(field: string, event: ReactPointerEvent<HTMLElement>) {
    if (event.button !== 0) return
    // The checkbox handles its own toggle; don't start a drag from it.
    if ((event.target as HTMLElement).closest('input[type="checkbox"]')) return
    dragStateRef.current = { field, startY: event.clientY, moved: false }
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  }

  function onRowPointerMove(event: ReactPointerEvent<HTMLElement>) {
    const state = dragStateRef.current
    if (!state) return
    if (!state.moved) {
      if (Math.abs(event.clientY - state.startY) < 4) return
      state.moved = true
      setDragField(state.field)
    }
    const target = fieldUnderPointer(event.clientY)
    if (target && target !== state.field) reorder(state.field, target)
  }

  function onRowPointerUp(event: ReactPointerEvent<HTMLElement>) {
    const state = dragStateRef.current
    if (state) {
      try { (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId) } catch { /* already released */ }
      // A press without movement on the row body toggles selection.
      if (!state.moved && !(event.target as HTMLElement).closest('input[type="checkbox"]')) {
        toggleSelected(state.field)
      }
    }
    dragStateRef.current = null
    setDragField(null)
  }

  return (
    <div
      className="create-record-backdrop"
      role="presentation"
      onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}
    >
      <section
        className="create-record-dialog view-editor"
        role="dialog"
        aria-modal="true"
        aria-label={initial ? '编辑视图' : '新建视图'}
        onMouseDown={e => e.stopPropagation()}
        onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      >
        <div className="view-editor-body">
          {/* Name + type */}
          <label className="view-editor-field">
            <span>视图名称</span>
            <input
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="输入视图名称"
              autoFocus
            />
          </label>
          <div className="view-editor-field">
            <span>视图类型</span>
            <div className="view-kind-choice">
              {(['table', 'graph'] as const).map(k => (
                <button
                  key={k}
                  className={`btn ${kind === k ? 'btn-primary' : 'btn-outlined'}`}
                  onClick={() => setKind(k)}
                  disabled={!!initial}
                  title={initial ? '已创建的视图不可更改类型' : undefined}
                >
                  <Icon name={k} size={13} aria-hidden />
                  {k === 'table' ? '表格视图' : '图视图'}
                </button>
              ))}
            </div>
          </div>

          {/* Graph: relations first (excluded from field choices below). */}
          {kind === 'graph' && (
            <div className="view-editor-field">
              <span>关系（作为图中的连线）</span>
              <ul className="view-field-list">
                {availableRelations.map(relation => (
                  <li key={relation} className="view-field-row">
                    <label>
                      <input
                        type="checkbox"
                        checked={relations.includes(relation)}
                        onChange={() => {
                          setRelations(cur => toggle(cur, relation))
                          // Drop from field selection if it was chosen there.
                          setSelected(cur => {
                            if (!cur.has(relation)) return cur
                            const next = new Set(cur)
                            next.delete(relation)
                            return next
                          })
                        }}
                      />
                      {relation}
                    </label>
                  </li>
                ))}
                {availableRelations.length === 0 && (
                  <li className="view-editor-empty">该类型没有可用关系</li>
                )}
              </ul>
            </div>
          )}

          {/* Fields (columns for table; card fields for graph). Drag the grip
              handle to reorder; the row order is independent of checked state. */}
          <div className="view-editor-field">
            <span>{kind === 'graph' ? '显示字段' : '显示列'}（拖动排序）</span>
            <ul className="view-field-list" ref={listRef}>
              {fieldRows.map(field => (
                <li
                  key={field}
                  data-field={field}
                  className={`view-field-row${selected.has(field) ? ' selected' : ''}${dragField === field ? ' dragging' : ''}`}
                  onPointerDown={e => onRowPointerDown(field, e)}
                  onPointerMove={onRowPointerMove}
                  onPointerUp={onRowPointerUp}
                  onPointerCancel={onRowPointerUp}
                >
                  <span className="view-field-label">
                    <input
                      type="checkbox"
                      checked={selected.has(field)}
                      onChange={() => toggleSelected(field)}
                      onClick={e => e.stopPropagation()}
                    />
                    {field}
                  </span>
                  <Icon name="grip" size={13} className="view-field-grip" aria-hidden />
                </li>
              ))}
              {fieldRows.length === 0 && (
                <li className="view-editor-empty">该类型没有可选字段</li>
              )}
            </ul>
          </div>

          {/* Group filter */}
          <label className="view-editor-field">
            <span>分组过滤{kind === 'graph' ? '（仅筛选根节点）' : ''}</span>
            <select
              value={groupFilter ?? ''}
              onChange={e => setGroupFilter(e.target.value || null)}
              disabled={groups.length === 0}
            >
              <option value="">（不过滤）</option>
              {groups.map(group => (
                <option key={group.id} value={group.id}>{group.name}</option>
              ))}
            </select>
          </label>
        </div>

        <div className="view-editor-actions">
          <button className="btn btn-outlined" onClick={onClose}>取消</button>
          <button className="btn btn-primary" onClick={submit} disabled={!trimmedName}>
            {initial ? '保存' : '创建'}
          </button>
        </div>
      </section>
    </div>
  )
}
