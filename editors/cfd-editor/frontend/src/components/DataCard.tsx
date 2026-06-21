import { useState, type CSSProperties } from 'react'
import type { FieldValue, FieldCell, DictKey } from '../bindings/index'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

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
  onEdit?: (fieldName: string, newValue: string) => void
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
          onEdit={onEdit ? val => onEdit(fc.name, val) : undefined}
          pathKey={pathPrefix ? `${pathPrefix}.${fc.name}` : fc.name}
          onRowToggle={onRowToggle}
        />
      ))}
    </div>
  )
}

function FieldRow({ label, value, depth, onEdit, pathKey, onRowToggle }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (newValue: string) => void
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
        pathKey={pathKey}
        onRowToggle={onRowToggle}
      />
    )
  }
  return <ScalarFieldRow label={label} value={value} depth={depth} onEdit={onEdit} pathKey={pathKey} />
}

function ScalarFieldRow({ label, value, depth, onEdit, pathKey }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (newValue: string) => void
  pathKey?: string
}) {
  const [editing, setEditing] = useState(false)
  const [editVal, setEditVal] = useState('')
  const isScalar = value.kind === 'Bool' || value.kind === 'Int' || value.kind === 'Float'
                || value.kind === 'Str' || value.kind === 'Enum'

  function startEdit() {
    if (!onEdit || !isScalar) return
    setEditVal(plainText(value))
    setEditing(true)
  }

  function commitEdit() {
    onEdit?.(editVal)
    setEditing(false)
  }

  return (
    <div className="dc-row" data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey}>
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
        {label}
      </div>
      <div className="dc-row-value">
        {editing ? (
          <input
            className="dc-input"
            value={editVal}
            autoFocus
            onChange={e => setEditVal(e.target.value)}
            onBlur={commitEdit}
            onKeyDown={e => {
              if (e.key === 'Enter') commitEdit()
              if (e.key === 'Escape') setEditing(false)
            }}
          />
        ) : (
          <div
            className={`dc-row-value-inner${isScalar && onEdit ? ' editable' : ''}`}
            onDoubleClick={startEdit}
            title={isScalar && onEdit ? '双击编辑' : undefined}
          >
            <ValueChip value={value} />
          </div>
        )}
      </div>
    </div>
  )
}

function ExpandableRow({ label, value, depth, pathKey, onRowToggle }: {
  label: string
  value: FieldValue
  depth: number
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const summary = headerSummary(value)
  const count = childCount(value)

  function toggle() {
    const next = !expanded
    setExpanded(next)
    if (pathKey) onRowToggle?.(pathKey, next)
  }

  return (
    <>
      <div className="dc-row dc-row-foldout" data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} onClick={toggle}>
        <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX }}>
          <span className="dc-fold-arrow">
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
          </span>
          {label}
        </div>
        <div className="dc-row-value">
          <div className="dc-row-value-inner">
            <span className="vc vc-type">{summary}</span>
            {count !== null && <span className="vc-count">{count}</span>}
          </div>
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
                pathKey={pathKey ? `${pathKey}.${fc.name}` : fc.name}
                onRowToggle={onRowToggle}
              />
            ))}
          {value.kind === 'Array' &&
            value.items.map((item, i) => (
              <FieldRow
                key={i}
                label={`Element ${i}`}
                value={item}
                depth={depth + 1}
                pathKey={pathKey ? `${pathKey}[${i}]` : `[${i}]`}
                onRowToggle={onRowToggle}
              />
            ))}
          {value.kind === 'Dict' &&
            value.entries.map((e, i) => (
              <FieldRow
                key={i}
                label={dictKeyText(e.key)}
                value={e.value}
                depth={depth + 1}
                pathKey={pathKey ? `${pathKey}[${dictKeyText(e.key)}]` : `[${dictKeyText(e.key)}]`}
                onRowToggle={onRowToggle}
              />
            ))}
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

function plainText(v: FieldValue): string {
  switch (v.kind) {
    case 'Bool':  return v.v ? 'true' : 'false'
    case 'Int':   return String(v.v)
    case 'Float': return String(v.v)
    case 'Str':   return v.v
    case 'Enum':  return v.variant
    default:      return ''
  }
}

// ─── Node mode (GraphView) ────────────────────────────────────────────────────

export function DataCardNode({
  fields,
  showAll,
  onToggle,
  onRowToggle,
}: {
  fields: FieldCell[]
  showAll: boolean
  onToggle: () => void
  onRowToggle?: (path: string, expanded: boolean) => void
}) {
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return (
    <div className="dc-node-card">
      <DataCardExpanded fields={visible} onRowToggle={onRowToggle} />
      {fields.length > NODE_PEEK_FIELDS && (
        <button className="dc-node-more" onClick={onToggle}>
          {showAll ? '收起' : `显示全部 (+${fields.length - NODE_PEEK_FIELDS})`}
        </button>
      )}
    </div>
  )
}
