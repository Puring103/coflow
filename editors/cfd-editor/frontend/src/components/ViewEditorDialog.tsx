import { useMemo, useState } from 'react'
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

/** Move an item within a list, returning a new array. */
function move<T>(list: T[], from: number, to: number): T[] {
  if (from === to || to < 0 || to >= list.length) return list
  const next = list.slice()
  const [item] = next.splice(from, 1)
  next.splice(to, 0, item)
  return next
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
  // Ordered selections; seed from initial (columns for table, fields for graph).
  const [fields, setFields] = useState<string[]>(
    initial ? (initial.kind === 'table' ? initial.columns : initial.fields) : [],
  )
  const [relations, setRelations] = useState<string[]>(initial?.relations ?? [])
  const [groupFilter, setGroupFilter] = useState<string | null>(initial?.group_filter ?? null)
  const [dragIndex, setDragIndex] = useState<number | null>(null)

  const trimmedName = name.trim()
  const relationSet = useMemo(() => new Set(relations), [relations])
  // Graph views: fields exclude already-selected relations (a relation is
  // shown as an edge, not a card field).
  const fieldChoices = useMemo(
    () => (kind === 'graph' ? availableFields.filter(f => !relationSet.has(f)) : availableFields),
    [availableFields, kind, relationSet],
  )
  // Selected first (in chosen order), then the rest — so the order is visible
  // and drag-reorderable.
  const selectedFields = useMemo(
    () => fields.filter(f => fieldChoices.includes(f)),
    [fields, fieldChoices],
  )
  const unselectedFields = useMemo(
    () => fieldChoices.filter(f => !fields.includes(f)),
    [fieldChoices, fields],
  )

  function submit() {
    if (!trimmedName) return
    const cleanFields = selectedFields
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

  function onDrop(targetIndex: number) {
    if (dragIndex === null) return
    setFields(cur => {
      const ordered = cur.filter(f => fieldChoices.includes(f))
      const rest = cur.filter(f => !fieldChoices.includes(f))
      return [...move(ordered, dragIndex, targetIndex), ...rest]
    })
    setDragIndex(null)
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
                          setFields(cur => cur.filter(f => f !== relation))
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

          {/* Fields (columns for table; card fields for graph). Drag to order. */}
          <div className="view-editor-field">
            <span>{kind === 'graph' ? '显示字段' : '显示列'}（拖动排序）</span>
            <ul className="view-field-list">
              {selectedFields.map((field, index) => (
                <li
                  key={field}
                  className={`view-field-row selected${dragIndex === index ? ' dragging' : ''}`}
                  draggable
                  onDragStart={() => setDragIndex(index)}
                  onDragOver={e => e.preventDefault()}
                  onDrop={() => onDrop(index)}
                  onDragEnd={() => setDragIndex(null)}
                >
                  <label>
                    <input
                      type="checkbox"
                      checked
                      onChange={() => setFields(cur => cur.filter(f => f !== field))}
                    />
                    {field}
                  </label>
                  <Icon name="grip" size={13} className="view-field-grip" aria-hidden />
                </li>
              ))}
              {unselectedFields.map(field => (
                <li key={field} className="view-field-row">
                  <label>
                    <input
                      type="checkbox"
                      checked={false}
                      onChange={() => setFields(cur => toggle(cur, field))}
                    />
                    {field}
                  </label>
                </li>
              ))}
              {fieldChoices.length === 0 && (
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
