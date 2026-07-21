import { useMemo, useState } from 'react'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { ViewConfig } from '../bindings/ViewConfig'
import type { ViewKind } from '../bindings/ViewKind'
import { newViewId } from '../state/views'
import { Icon } from './Icon'

interface Props {
  /** null id => create mode; an existing ViewConfig => edit mode. */
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

type Tab = 'type' | 'fields' | 'relations' | 'groups'

/** Move an item within a list, returning a new array. */
function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length) return list
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
  const [tab, setTab] = useState<Tab>('type')
  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<ViewKind>(initial?.kind ?? 'table')
  // Ordered selections; seed from initial (columns for table, fields for graph).
  const [fields, setFields] = useState<string[]>(
    initial ? (initial.kind === 'table' ? initial.columns : initial.fields) : [],
  )
  const [relations, setRelations] = useState<string[]>(initial?.relations ?? [])
  const [groupFilter, setGroupFilter] = useState<string | null>(initial?.group_filter ?? null)

  const trimmedName = name.trim()
  // Selected first (in order), then the rest, so ordering is visible/editable.
  const orderedFieldChoices = useMemo(() => {
    const rest = availableFields.filter(f => !fields.includes(f))
    return [...fields, ...rest]
  }, [availableFields, fields])

  function submit() {
    if (!trimmedName) { setTab('type'); return }
    const base: ViewConfig = {
      id: initial?.id ?? newViewId(),
      name: trimmedName,
      kind,
      group_filter: groupFilter,
      columns: kind === 'table' ? fields : [],
      column_widths: kind === 'table' ? (initial?.column_widths ?? {}) : {},
      relations: kind === 'graph' ? relations : [],
      fields: kind === 'graph' ? fields : [],
    }
    onSubmit(base)
    onClose()
  }

  const tabs: { id: Tab; label: string }[] = [
    { id: 'type', label: '类型' },
    { id: 'fields', label: '字段' },
    ...(kind === 'graph' ? [{ id: 'relations' as Tab, label: '关系' }] : []),
    { id: 'groups', label: '分组' },
  ]

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
        <div className="view-editor-tabs" role="tablist">
          {tabs.map(t => (
            <button
              key={t.id}
              className={`tab-btn${tab === t.id ? ' active' : ''}`}
              role="tab"
              aria-selected={tab === t.id}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>

        <div className="view-editor-body">
          {tab === 'type' && (
            <div className="view-editor-pane">
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
            </div>
          )}

          {tab === 'fields' && (
            <div className="view-editor-pane">
              <p className="view-editor-hint">勾选要显示的字段，拖动或用箭头调整顺序。</p>
              <ul className="view-field-list">
                {orderedFieldChoices.map(field => {
                  const selected = fields.includes(field)
                  const index = fields.indexOf(field)
                  return (
                    <li key={field} className={`view-field-row${selected ? ' selected' : ''}`}>
                      <label>
                        <input
                          type="checkbox"
                          checked={selected}
                          onChange={() => setFields(cur => toggle(cur, field))}
                        />
                        {field}
                      </label>
                      {selected && (
                        <span className="view-field-order">
                          <button
                            className="btn btn-icon"
                            disabled={index <= 0}
                            onClick={() => setFields(cur => move(cur, index, index - 1))}
                            aria-label="上移"
                          >
                            <Icon name="arrow-up" size={12} aria-hidden />
                          </button>
                          <button
                            className="btn btn-icon"
                            disabled={index >= fields.length - 1}
                            onClick={() => setFields(cur => move(cur, index, index + 1))}
                            aria-label="下移"
                          >
                            <Icon name="arrow-down" size={12} aria-hidden />
                          </button>
                        </span>
                      )}
                    </li>
                  )
                })}
                {orderedFieldChoices.length === 0 && (
                  <li className="view-editor-empty">该类型没有可选字段</li>
                )}
              </ul>
            </div>
          )}

          {tab === 'relations' && kind === 'graph' && (
            <div className="view-editor-pane">
              <p className="view-editor-hint">勾选要在图中显示的关系边。</p>
              <ul className="view-field-list">
                {availableRelations.map(relation => (
                  <li key={relation} className="view-field-row">
                    <label>
                      <input
                        type="checkbox"
                        checked={relations.includes(relation)}
                        onChange={() => setRelations(cur => toggle(cur, relation))}
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

          {tab === 'groups' && (
            <div className="view-editor-pane">
              <p className="view-editor-hint">
                {kind === 'graph'
                  ? '选择一个分组作为图的根节点范围（仅筛选根节点）。'
                  : '选择一个分组以仅显示该分组的记录。'}
              </p>
              <label className="view-editor-field">
                <span>分组过滤</span>
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
              {groups.length === 0 && (
                <p className="view-editor-empty">该类型还没有分组</p>
              )}
            </div>
          )}
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
