import { useState, useEffect, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react'
import type { FieldValue, FieldCell, DictKey, FieldPathSegment } from '../bindings/index'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import { loadEnumVariants, loadRefTargets, buildDefaultObject } from '../utils/editContext'

// ─── Shared card header (used in graph nodes, record view, table detail) ─────

export function CardHeader({
  recordKey,
  actualType,
  filePath,
}: {
  recordKey: string
  actualType: string
  filePath?: string
}) {
  const color = typeColor(actualType)
  return (
    <div className="gn-header" style={{ '--node-color': color } as CSSProperties}>
      <div className="gn-color-bar" />
      <span className="gn-key">{recordKey}</span>
      <div className="gn-meta">
        <span className="gn-type">{actualType}</span>
        {filePath && <span className="gn-file">{filePath.split('/').pop()}</span>}
      </div>
    </div>
  )
}

export const NODE_PEEK_FIELDS = 4
const MAX_DEPTH = 5
const INDENT_PX = 14

// ─── Type / kind labels ──────────────────────────────────────────────────────

function valueKindLabel(v: FieldValue): string {
  switch (v.kind) {
    case 'Null':   return 'null'
    case 'Bool':   return 'bool'
    case 'Int':    return 'int'
    case 'Float':  return 'float'
    case 'Str':    return 'string'
    case 'Enum':   return v.enum_name
    case 'Object': return v.actual_type
    case 'Ref':    return v.target_type
    case 'Array':  return v.items[0] ? `${valueKindLabel(v.items[0])}[]` : '[]'
    case 'Dict':   return v.entries[0]
      ? `{${dictKindLabel(v.entries[0].key)}:${valueKindLabel(v.entries[0].value)}}`
      : '{}'
  }
}

function dictKindLabel(k: DictKey): string {
  switch (k.kind) {
    case 'Str':  return 'string'
    case 'Int':  return 'int'
    case 'Enum': return k.enum_name
  }
}

function dictKeyText(k: DictKey): string {
  switch (k.kind) {
    case 'Str':  return `"${k.v}"`
    case 'Int':  return String(k.v)
    case 'Enum': return k.variant
  }
}

/** AST-form of a dict key: the parser stores dict entries as Block fields whose
 * name is the unquoted text (string keys without quotes, ints as digits, enum
 * variants as identifiers). Backend path navigation matches by this form. */
function dictKeyAstName(k: DictKey): string {
  switch (k.kind) {
    case 'Str':  return k.v
    case 'Int':  return String(k.v)
    case 'Enum': return k.variant
  }
}

// ─── Compact summary text ────────────────────────────────────────────────────

export function summaryOf(v: FieldValue): string {
  switch (v.kind) {
    case 'Null':  return '—'
    case 'Bool':  return v.v ? 'true' : 'false'
    case 'Int':   return String(v.v)
    case 'Float': return String(v.v)
    case 'Str':   return v.v.length > 32 ? `"${v.v.slice(0, 30)}…"` : `"${v.v}"`
    case 'Enum':  return v.variant
    case 'Ref':   return v.target_key
    case 'Object': return v.actual_type
    case 'Array': {
      if (v.items.length === 0) return '[]'
      const allScalar = v.items.every(i =>
        i.kind === 'Bool' || i.kind === 'Int' || i.kind === 'Float' || i.kind === 'Str' || i.kind === 'Enum'
      )
      if (allScalar && v.items.length <= 6) {
        const joined = v.items.map(summaryOf).join(', ')
        if (joined.length <= 60) return `[${joined}]`
      }
      return `${valueKindLabel(v.items[0])}[${v.items.length}]`
    }
    case 'Dict': {
      if (v.entries.length === 0) return '{}'
      const first = v.entries[0]
      return `${dictKindLabel(first.key)}→${valueKindLabel(first.value)}  (${v.entries.length})`
    }
  }
}

// ─── Count visible rows (for height estimation in GraphView) ─────────────────
// Recursively counts how many rows would be rendered given the expanded paths.

export function countVisibleRows(
  fields: FieldCell[],
  expandedPaths: Set<string>,
  prefix = '',
): number {
  let count = 0
  for (const f of fields) {
    count++
    const path = prefix ? `${prefix}.${f.name}` : f.name
    if (!expandedPaths.has(path)) continue
    if (f.value.kind === 'Object') {
      count += countVisibleRows(f.value.fields, expandedPaths, path)
    } else if (f.value.kind === 'Array') {
      count += f.value.items.length
    } else if (f.value.kind === 'Dict') {
      count += f.value.entries.length
    }
  }
  return count
}

// ─── Compact cell (used inside TableView) ────────────────────────────────────

export function DataCardCompact({ value }: { value: FieldValue }) {
  return <ValueChip value={value} />
}

function ValueChip({ value }: { value: FieldValue }) {
  switch (value.kind) {
    case 'Null':
      return <span className="vc vc-null">null</span>
    case 'Bool':
      return <span className={`vc vc-bool${value.v ? ' on' : ''}`}>{value.v ? 'true' : 'false'}</span>
    case 'Int':
    case 'Float':
      return <span className="vc vc-num">{value.kind === 'Int' ? value.v : value.v}</span>
    case 'Str':
      return <span className="vc vc-str">{summaryOf(value)}</span>
    case 'Enum':
      return (
        <span className="vc vc-enum">
          <span className="vc-enum-dot" />
          {value.variant}
        </span>
      )
    case 'Ref':
      return (
        <span className="vc vc-ref" title={`${value.target_type}.${value.target_key}`}>
          <Icon name="dot" size={9} />
          <span className="vc-ref-key">{value.target_key}</span>
          <span className="vc-ref-type">({value.target_type})</span>
        </span>
      )
    case 'Object':
      return <span className="vc vc-obj">{value.actual_type}</span>
    case 'Array':
      return <span className="vc vc-arr">{summaryOf(value)}</span>
    case 'Dict':
      return <span className="vc vc-dict">{summaryOf(value)}</span>
  }
}

// ─── Expanded inspector (RecordView / TableView detail) ───────────────────────

export interface ExpandedProps {
  fields: FieldCell[]
  depth?: number
  /** Called with the full path from the record root (Field/Index segments). */
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  pathPrefix?: string
  onRowToggle?: (path: string, expanded: boolean) => void
}

export function DataCardExpanded({ fields, depth = 0, onEdit, pathPrefix, onRowToggle }: ExpandedProps) {
  return (
    <div className="dc-inspector" style={{ '--depth': depth } as CSSProperties}>
      {fields.map((fc, i) => (
        <FieldRow
          key={fc.name + i}
          label={fc.name}
          value={fc.value}
          depth={depth}
          // Spread-introduced fields are not present in this record's source
          // text — they belong to a different record. Disable editing.
          onEdit={fc.is_spread ? undefined : onEdit}
          isSpread={fc.is_spread}
          fieldPath={[{ kind: 'field', name: fc.name }]}
          pathKey={pathPrefix ? `${pathPrefix}.${fc.name}` : fc.name}
          onRowToggle={onRowToggle}
        />
      ))}
    </div>
  )
}

function FieldRow({ label, value, depth, onEdit, isSpread, fieldPath, pathKey, onRowToggle }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  isSpread?: boolean
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
}) {
  const isComplex = value.kind === 'Object' || value.kind === 'Array' || value.kind === 'Dict'
  const canExpand = isComplex && depth < MAX_DEPTH

  if (canExpand) {
    return (
      <ExpandableRow
        label={label}
        value={value}
        depth={depth}
        onEdit={onEdit}
        isSpread={isSpread}
        fieldPath={fieldPath}
        pathKey={pathKey}
        onRowToggle={onRowToggle}
      />
    )
  }
  return (
    <ScalarFieldRow
      label={label}
      value={value}
      depth={depth}
      onCommit={onEdit ? next => onEdit(fieldPath, next) : undefined}
      isSpread={isSpread}
      pathKey={pathKey}
    />
  )
}

function ScalarFieldRow({ label, value, depth, onCommit, isSpread, pathKey }: {
  label: string
  value: FieldValue
  depth: number
  onCommit?: (newValue: FieldValue) => void
  isSpread?: boolean
  pathKey?: string
}) {
  const [editing, setEditing] = useState(false)
  const isScalar = value.kind === 'Bool' || value.kind === 'Int' || value.kind === 'Float'
                || value.kind === 'Str' || value.kind === 'Enum' || value.kind === 'Ref'
  const canEdit = isScalar && !!onCommit

  return (
    <div className={`dc-row${isSpread ? ' dc-row-spread' : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} title={isSpread ? '此字段来自 ...spread 展开，需到源记录编辑' : undefined}>
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
        {label}
      </div>
      <div className="dc-row-value">
        {editing && canEdit ? (
          <InlineEditor
            value={value}
            onCommit={next => { onCommit!(next); setEditing(false) }}
            onCancel={() => setEditing(false)}
          />
        ) : (
          <div
            className={`dc-row-value-inner${canEdit ? ' editable' : ''}`}
            onDoubleClick={canEdit ? () => setEditing(true) : undefined}
            title={canEdit ? '双击编辑' : undefined}
          >
            <ValueChip value={value} />
            {onCommit && value.kind === 'Ref' && (
              <button
                className="btn-tiny dc-row-mode-btn"
                title="切换为内联对象（用 schema 默认值新建）"
                onClick={async e => {
                  e.stopPropagation()
                  const obj = await buildDefaultObject(value.target_type)
                  if (obj) onCommit(obj)
                }}
              >→Inline</button>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

/// Standalone inline editor: picks the right input widget by kind and emits the
/// fully-typed FieldValue. Used by RecordView/TableView detail panel and table cells.
export function InlineEditor({
  value, onCommit, onCancel,
}: {
  value: FieldValue
  onCommit: (next: FieldValue) => void
  onCancel: () => void
}) {
  const initial = plainText(value)
  const [editVal, setEditVal] = useState(initial)

  function commit(raw: string) {
    const next = buildFieldValue(value, raw)
    if (next) onCommit(next)
    else onCancel()
  }

  if (value.kind === 'Bool') {
    return (
      <select
        className="dc-input"
        value={editVal}
        autoFocus
        onChange={e => commit(e.target.value)}
        onBlur={onCancel}
        onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
      >
        <option value="true">true</option>
        <option value="false">false</option>
      </select>
    )
  }
  if (value.kind === 'Enum') {
    return <EnumSelect enumName={value.enum_name} current={editVal} onCommit={commit} onCancel={onCancel} />
  }
  if (value.kind === 'Ref') {
    return <RefSelect targetType={value.target_type} current={editVal} onCommit={commit} onCancel={onCancel} />
  }
  return (
    <input
      className="dc-input"
      type={value.kind === 'Int' || value.kind === 'Float' ? 'number' : 'text'}
      value={editVal}
      autoFocus
      onChange={e => setEditVal(e.target.value)}
      onBlur={() => commit(editVal)}
      onKeyDown={e => {
        if (e.key === 'Enter') commit(editVal)
        if (e.key === 'Escape') onCancel()
      }}
    />
  )
}

function EnumSelect({
  enumName, current, onCommit, onCancel,
}: {
  enumName: string
  current: string
  onCommit: (v: string) => void
  onCancel: () => void
}) {
  const [variants, setVariants] = useState<string[] | null>(null)
  useEffect(() => {
    let alive = true
    loadEnumVariants(enumName).then(v => { if (alive) setVariants(v ?? []) })
    return () => { alive = false }
  }, [enumName])

  if (variants === null) {
    return <input className="dc-input" value={current} disabled placeholder="加载中…" />
  }
  if (variants.length === 0) {
    return (
      <input
        className="dc-input"
        defaultValue={current}
        autoFocus
        onBlur={e => onCommit(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') onCommit((e.target as HTMLInputElement).value)
          if (e.key === 'Escape') onCancel()
        }}
      />
    )
  }
  return (
    <select
      className="dc-input"
      defaultValue={current}
      autoFocus
      onChange={e => onCommit(e.target.value)}
      onBlur={onCancel}
      onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
    >
      {!variants.includes(current) && <option value={current}>{current}</option>}
      {variants.map(v => <option key={v} value={v}>{v}</option>)}
    </select>
  )
}

function RefSelect({
  targetType, current, onCommit, onCancel,
}: {
  targetType: string
  current: string
  onCommit: (v: string) => void
  onCancel: () => void
}) {
  const [targets, setTargets] = useState<string[] | null>(null)
  const [val, setVal] = useState(current)
  useEffect(() => {
    let alive = true
    loadRefTargets(targetType).then(v => { if (alive) setTargets(v ?? []) })
    return () => { alive = false }
  }, [targetType])

  const listId = `ref-targets-${targetType}`
  return (
    <>
      <input
        className="dc-input"
        list={listId}
        value={val}
        autoFocus
        placeholder={targets === null ? '加载中…' : `${targetType} key`}
        onChange={e => setVal(e.target.value)}
        onBlur={() => onCommit(val)}
        onKeyDown={e => {
          if (e.key === 'Enter') onCommit(val)
          if (e.key === 'Escape') onCancel()
        }}
      />
      {targets && targets.length > 0 && (
        <datalist id={listId}>
          {targets.map(t => <option key={t} value={t} />)}
        </datalist>
      )}
    </>
  )
}

function ExpandableRow({ label, value, depth, onEdit, isSpread, fieldPath, pathKey, onRowToggle }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  isSpread?: boolean
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const [pickingRef, setPickingRef] = useState(false)
  const summary = headerSummary(value)
  const count = childCount(value)

  function toggle() {
    const next = !expanded
    setExpanded(next)
    if (pathKey) onRowToggle?.(pathKey, next)
  }

  return (
    <>
      <div className={`dc-row dc-row-foldout${isSpread ? ' dc-row-spread' : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} title={isSpread ? '此字段来自 ...spread 展开，需到源记录编辑' : undefined} onClick={toggle}>
        <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX }}>
          <span className="dc-fold-arrow">
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
          </span>
          {label}
        </div>
        <div className="dc-row-value">
          {pickingRef && value.kind === 'Object' ? (
            <span onClick={e => e.stopPropagation()}>
              <RefSelect
                targetType={value.actual_type}
                current=""
                onCommit={key => {
                  setPickingRef(false)
                  if (!key) return
                  onEdit?.(fieldPath, {
                    kind: 'Ref', target_type: value.actual_type, target_key: key, target_file: null,
                  })
                }}
                onCancel={() => setPickingRef(false)}
              />
            </span>
          ) : (
            <div className="dc-row-value-inner">
              <span className="vc vc-type">{summary}</span>
              {count !== null && <span className="vc-count">{count}</span>}
              {onEdit && value.kind === 'Object' && (
                <button
                  className="btn-tiny dc-row-mode-btn"
                  title="切换为引用（指向已有同类型记录）"
                  onClick={e => { e.stopPropagation(); setPickingRef(true) }}
                >→Ref</button>
              )}
            </div>
          )}
        </div>
      </div>
      {expanded && (
        <>
          {value.kind === 'Object' &&
            value.fields.map((fc, i) => (
              <FieldRow
                key={fc.name + i}
                label={fc.name}
                value={fc.value}
                depth={depth + 1}
                onEdit={onEdit}
                fieldPath={[...fieldPath, { kind: 'field', name: fc.name }]}
                pathKey={pathKey ? `${pathKey}.${fc.name}` : fc.name}
                onRowToggle={onRowToggle}
              />
            ))}
          {value.kind === 'Array' &&
            value.items.map((item, i) => (
              <div key={i} className="dc-collection-row">
                <FieldRow
                  label={`[${i}]`}
                  value={item}
                  depth={depth + 1}
                  onEdit={onEdit}
                  fieldPath={[...fieldPath, { kind: 'index', i }]}
                  pathKey={pathKey ? `${pathKey}[${i}]` : `[${i}]`}
                  onRowToggle={onRowToggle}
                />
                {onEdit && (
                  <ArrayItemControls
                    index={i}
                    total={value.items.length}
                    onMove={(from, to) => onEdit(fieldPath, arrayMove(value, from, to))}
                    onRemove={i => onEdit(fieldPath, arrayRemove(value, i))}
                  />
                )}
              </div>
            ))}
          {value.kind === 'Dict' &&
            value.entries.map((e, i) => (
              <div key={i} className="dc-collection-row">
                <FieldRow
                  label={dictKeyText(e.key)}
                  value={e.value}
                  depth={depth + 1}
                  onEdit={onEdit}
                  fieldPath={[...fieldPath, { kind: 'field', name: dictKeyAstName(e.key) }]}
                  pathKey={pathKey ? `${pathKey}[${dictKeyText(e.key)}]` : `[${dictKeyText(e.key)}]`}
                  onRowToggle={onRowToggle}
                />
                {onEdit && (
                  <DictItemControls
                    onRemove={() => onEdit(fieldPath, dictRemove(value, e.key))}
                  />
                )}
              </div>
            ))}
          {onEdit && (value.kind === 'Array' || value.kind === 'Dict') && (
            <CollectionAddRow
              container={value}
              depth={depth + 1}
              onAdd={next => onEdit(fieldPath, next)}
            />
          )}
          {value.kind === 'Array' && value.items.length === 0 && (
            <EmptyHint depth={depth + 1} text="空数组" />
          )}
          {value.kind === 'Dict' && value.entries.length === 0 && (
            <EmptyHint depth={depth + 1} text="空字典" />
          )}
        </>
      )}
    </>
  )
}

function EmptyHint({ depth, text }: { depth: number; text: string }) {
  return (
    <div className="dc-row dc-row-empty">
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 8 }} />
      <div className="dc-row-value">
        <span className="vc vc-null">{text}</span>
      </div>
    </div>
  )
}

function headerSummary(v: FieldValue): string {
  switch (v.kind) {
    case 'Object': return v.actual_type
    case 'Array':  return v.items[0] ? `${valueKindLabel(v.items[0])}[]` : 'array'
    case 'Dict':   return v.entries[0]
      ? `${dictKindLabel(v.entries[0].key)} → ${valueKindLabel(v.entries[0].value)}`
      : 'dict'
    default:       return ''
  }
}

function childCount(v: FieldValue): number | null {
  switch (v.kind) {
    case 'Array': return v.items.length
    case 'Dict':  return v.entries.length
    default:      return null
  }
}

// ─── Collection mutations (array/dict) ───────────────────────────────────────
// Build a *new full collection* value with the change applied. The caller then
// calls onEdit(fieldPath_to_collection, newCollection); the backend's existing
// span-patch writer replaces the whole collection.

function arrayMove(arr: FieldValue & { kind: 'Array' }, from: number, to: number): FieldValue {
  if (from === to || from < 0 || to < 0 || from >= arr.items.length || to >= arr.items.length) {
    return arr
  }
  const items = arr.items.slice()
  const [moved] = items.splice(from, 1)
  items.splice(to, 0, moved)
  return { kind: 'Array', items }
}

function arrayRemove(arr: FieldValue & { kind: 'Array' }, i: number): FieldValue {
  const items = arr.items.slice()
  items.splice(i, 1)
  return { kind: 'Array', items }
}

function arrayAppend(arr: FieldValue & { kind: 'Array' }, value: FieldValue): FieldValue {
  return { kind: 'Array', items: [...arr.items, value] }
}

function dictRemove(d: FieldValue & { kind: 'Dict' }, key: DictKey): FieldValue {
  return { kind: 'Dict', entries: d.entries.filter(e => !dictKeyEq(e.key, key)) }
}

function dictInsert(d: FieldValue & { kind: 'Dict' }, key: DictKey, value: FieldValue): FieldValue {
  // If the key already exists, replace its value; otherwise append.
  const idx = d.entries.findIndex(e => dictKeyEq(e.key, key))
  if (idx >= 0) {
    const entries = d.entries.slice()
    entries[idx] = { key, value }
    return { kind: 'Dict', entries }
  }
  return { kind: 'Dict', entries: [...d.entries, { key, value }] }
}

function dictKeyEq(a: DictKey, b: DictKey): boolean {
  if (a.kind !== b.kind) return false
  if (a.kind === 'Str' && b.kind === 'Str') return a.v === b.v
  if (a.kind === 'Int' && b.kind === 'Int') return a.v === b.v
  if (a.kind === 'Enum' && b.kind === 'Enum') return a.enum_name === b.enum_name && a.variant === b.variant
  return false
}

/** Default value for a brand-new collection element, derived from a sibling.
 * If the collection has no existing items we fall back to `Null`; the user
 * can then double-click to enter a value and the parser will need a concrete
 * type, so this is best-effort. */
function defaultElementFor(container: FieldValue): FieldValue {
  // Find the first non-null sample to derive shape from. Falling back to a
  // Null sample would emit `null` which most schemas reject.
  if (container.kind === 'Array') {
    const sample = container.items.find(i => i.kind !== 'Null') ?? container.items[0]
    if (sample) return defaultLikeShape(sample)
  }
  if (container.kind === 'Dict') {
    const sample = container.entries.find(e => e.value.kind !== 'Null')?.value
      ?? container.entries[0]?.value
    if (sample) return defaultLikeShape(sample)
  }
  return { kind: 'Str', v: '' }
}

function defaultLikeShape(sample: FieldValue): FieldValue {
  switch (sample.kind) {
    case 'Bool':  return { kind: 'Bool', v: false }
    case 'Int':   return { kind: 'Int', v: 0 }
    case 'Float': return { kind: 'Float', v: 0 }
    case 'Str':   return { kind: 'Str', v: '' }
    case 'Null':  return { kind: 'Str', v: '' }  // can't keep null as default — schema usually disallows
    case 'Enum':  return { kind: 'Enum', enum_name: sample.enum_name, variant: sample.variant, int_value: sample.int_value }
    case 'Ref':   return { kind: 'Ref', target_type: sample.target_type, target_key: '', target_file: null }
    case 'Object': return { kind: 'Object', actual_type: sample.actual_type, fields: sample.fields.map(f => ({ name: f.name, value: defaultLikeShape(f.value) })) }
    case 'Array': return { kind: 'Array', items: [] }
    case 'Dict':  return { kind: 'Dict', entries: [] }
  }
}

function ArrayItemControls({ index, total, onMove, onRemove }: {
  index: number
  total: number
  onMove: (from: number, to: number) => void
  onRemove: (i: number) => void
}) {
  const stop = (fn: () => void) => (e: ReactMouseEvent) => { e.stopPropagation(); fn() }
  return (
    <div className="dc-row-controls">
      <button className="btn-tiny" disabled={index === 0} title="上移" onClick={stop(() => onMove(index, index - 1))}>↑</button>
      <button className="btn-tiny" disabled={index === total - 1} title="下移" onClick={stop(() => onMove(index, index + 1))}>↓</button>
      <button className="btn-tiny btn-tiny-danger" title="删除" onClick={stop(() => onRemove(index))}>✕</button>
    </div>
  )
}

function DictItemControls({ onRemove }: { onRemove: () => void }) {
  return (
    <div className="dc-row-controls">
      <button
        className="btn-tiny btn-tiny-danger"
        title="删除"
        onClick={e => { e.stopPropagation(); onRemove() }}
      >✕</button>
    </div>
  )
}

function CollectionAddRow({ container, depth, onAdd }: {
  container: FieldValue & { kind: 'Array' | 'Dict' }
  depth: number
  onAdd: (next: FieldValue) => void
}) {
  const [adding, setAdding] = useState(false)

  function reset() { setAdding(false) }

  if (container.kind === 'Array') {
    // Sample first item's kind to decide whether to ask for a value first.
    const sample = container.items[0]
    const needsPicker = sample && (sample.kind === 'Enum' || sample.kind === 'Ref' || sample.kind === 'Bool')
    if (!needsPicker) {
      // Plain types — append a default and let the user double-click to edit.
      return (
        <div className="dc-row dc-row-add" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
          <button
            className="btn-add-item"
            onClick={() => onAdd(arrayAppend(container, defaultElementFor(container)))}
          >
            <Icon name="plus" size={11} /> 添加元素
          </button>
        </div>
      )
    }
    return (
      <div className="dc-row dc-row-add" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
        {!adding ? (
          <button className="btn-add-item" onClick={() => setAdding(true)}>
            <Icon name="plus" size={11} /> 添加元素
          </button>
        ) : (
          <span className="dc-add-form">
            <InlineEditor
              value={defaultLikeShape(sample!)}
              onCommit={v => { onAdd(arrayAppend(container, v)); reset() }}
              onCancel={reset}
            />
            <button className="btn-tiny" onClick={reset}>✕</button>
          </span>
        )}
      </div>
    )
  }

  // Dict
  if (container.kind !== 'Dict') return null  // unreachable; narrows for ts
  const sampleKey: DictKey = container.entries[0]?.key ?? { kind: 'Str', v: '' }
  function tryAdd(key: DictKey) {
    if (container.kind !== 'Dict') return
    const dup = container.entries.some(e => dictKeyEq(e.key, key))
    if (dup) {
      alert(`键 "${dictKeyText(key)}" 已存在`)
      return
    }
    onAdd(dictInsert(container, key, defaultElementFor(container)))
    reset()
  }
  return (
    <div className="dc-row dc-row-add" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
      {!adding ? (
        <button className="btn-add-item" onClick={() => setAdding(true)}>
          <Icon name="plus" size={11} /> 添加项
        </button>
      ) : (
        <DictKeyEntry
          sampleKey={sampleKey}
          onCommit={tryAdd}
          onCancel={reset}
        />
      )}
    </div>
  )
}

function DictKeyEntry({ sampleKey, onCommit, onCancel }: {
  sampleKey: DictKey
  onCommit: (k: DictKey) => void
  onCancel: () => void
}) {
  const [text, setText] = useState('')

  // Enum-keyed dict: load variants and present a select.
  const [variants, setVariants] = useState<string[] | null>(null)
  useEffect(() => {
    if (sampleKey.kind !== 'Enum') return
    let alive = true
    loadEnumVariants(sampleKey.enum_name).then(v => { if (alive) setVariants(v ?? []) })
    return () => { alive = false }
  }, [sampleKey.kind === 'Enum' ? sampleKey.enum_name : ''])

  if (sampleKey.kind === 'Enum') {
    if (variants === null) {
      return <span className="dc-add-form"><span className="dc-add-loading">加载枚举…</span></span>
    }
    if (variants.length === 0) {
      // Backend has no variants — fall back to text input.
      return (
        <span className="dc-add-form">
          <input
            className="dc-input" autoFocus value={text}
            placeholder="枚举变体"
            onChange={e => setText(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && text) onCommit({ kind: 'Enum', enum_name: sampleKey.enum_name, variant: text, int_value: 0 })
              if (e.key === 'Escape') onCancel()
            }}
          />
          <button className="btn-tiny" onClick={() => text && onCommit({ kind: 'Enum', enum_name: sampleKey.enum_name, variant: text, int_value: 0 })}>✓</button>
          <button className="btn-tiny" onClick={onCancel}>✕</button>
        </span>
      )
    }
    return (
      <span className="dc-add-form">
        <select
          className="dc-input"
          autoFocus
          defaultValue=""
          onChange={e => {
            if (e.target.value) onCommit({ kind: 'Enum', enum_name: sampleKey.enum_name, variant: e.target.value, int_value: 0 })
          }}
          onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
        >
          <option value="" disabled>选择…</option>
          {variants.map(v => <option key={v} value={v}>{v}</option>)}
        </select>
        <button className="btn-tiny" onClick={onCancel}>✕</button>
      </span>
    )
  }

  // Str / Int key entry.
  function commit() {
    if (!text) return
    if (sampleKey.kind === 'Int') {
      const n = parseInt(text, 10)
      if (!Number.isFinite(n)) return
      onCommit({ kind: 'Int', v: n })
    } else {
      onCommit({ kind: 'Str', v: text })
    }
  }
  return (
    <span className="dc-add-form">
      <input
        className="dc-input"
        placeholder={sampleKey.kind === 'Int' ? '整数 key' : '字符串 key'}
        autoFocus
        value={text}
        onChange={e => setText(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') commit()
          if (e.key === 'Escape') onCancel()
        }}
      />
      <button className="btn-tiny" onClick={commit}>✓</button>
      <button className="btn-tiny" onClick={onCancel}>✕</button>
    </span>
  )
}

// Build a FieldValue of the same kind as `original` from raw text input.
// Returns null if the text can't be parsed for the kind (caller cancels).
function buildFieldValue(original: FieldValue, raw: string): FieldValue | null {
  switch (original.kind) {
    case 'Bool':
      return { kind: 'Bool', v: raw === 'true' }
    case 'Int': {
      const n = parseInt(raw, 10)
      return Number.isFinite(n) ? { kind: 'Int', v: n } : null
    }
    case 'Float': {
      const n = parseFloat(raw)
      return Number.isFinite(n) ? { kind: 'Float', v: n } : null
    }
    case 'Str':
      return { kind: 'Str', v: raw }
    case 'Enum':
      return {
        kind: 'Enum',
        enum_name: original.enum_name,
        variant: raw,
        int_value: original.int_value,
      }
    case 'Ref':
      return {
        kind: 'Ref',
        target_type: original.target_type,
        target_key: raw,
        target_file: null,
      }
    default:
      return null
  }
}

function plainText(v: FieldValue): string {
  switch (v.kind) {
    case 'Bool':  return v.v ? 'true' : 'false'
    case 'Int':   return String(v.v)
    case 'Float': return String(v.v)
    case 'Str':   return v.v
    case 'Enum':  return v.variant
    case 'Ref':   return v.target_key
    default:      return ''
  }
}

// ─── Node mode (GraphView) ────────────────────────────────────────────────────

export function DataCardNode({
  fields,
  showAll,
  onToggle,
  onRowToggle,
  onEdit,
}: {
  fields: FieldCell[]
  showAll: boolean
  onToggle: () => void
  onRowToggle?: (path: string, expanded: boolean) => void
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
}) {
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return (
    <div className="dc-node-card">
      <DataCardExpanded fields={visible} onRowToggle={onRowToggle} onEdit={onEdit} />
      {fields.length > NODE_PEEK_FIELDS && (
        <button className="dc-node-more" onClick={onToggle}>
          {showAll ? '收起' : `显示全部 (+${fields.length - NODE_PEEK_FIELDS})`}
        </button>
      )}
    </div>
  )
}
