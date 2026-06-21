import { useState, type CSSProperties } from 'react'
import type { FieldValue, FieldCell, DictKey } from '../bindings/index'
import { Icon } from './Icon'

const MAX_DEPTH = 5
const INDENT_PX = 14

// ─── Type / kind labels ──────────────────────────────────────────────────

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

// ─── Compact summary text ────────────────────────────────────────────────

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

// ─── Compact cell (used inside TableView) ────────────────────────────────

export function DataCardCompact({ value }: { value: FieldValue }) {
  return <ValueChip value={value} />
}

// Inline value renderer — styled like a property field, no navigation.
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

// ─── Expanded inspector (RecordView) ─────────────────────────────────────

export interface ExpandedProps {
  fields: FieldCell[]
  depth?: number
  onEdit?: (fieldName: string, newValue: string) => void
}

export function DataCardExpanded({ fields, depth = 0, onEdit }: ExpandedProps) {
  return (
    <div className="dc-inspector" style={{ '--depth': depth } as CSSProperties}>
      {fields.map((fc, i) => (
        <FieldRow
          key={fc.name + i}
          label={fc.name}
          value={fc.value}
          depth={depth}
          onEdit={onEdit ? val => onEdit(fc.name, val) : undefined}
        />
      ))}
    </div>
  )
}

function FieldRow({ label, value, depth, onEdit }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (newValue: string) => void
}) {
  const isComplex = value.kind === 'Object' || value.kind === 'Array' || value.kind === 'Dict'
  const canExpand = isComplex && depth < MAX_DEPTH

  if (canExpand) {
    return <ExpandableRow label={label} value={value} depth={depth} />
  }
  return <ScalarFieldRow label={label} value={value} depth={depth} onEdit={onEdit} />
}

function ScalarFieldRow({ label, value, depth, onEdit }: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (newValue: string) => void
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
    <div className="dc-row" data-depth={depth}>
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

function ExpandableRow({ label, value, depth }: {
  label: string
  value: FieldValue
  depth: number
}) {
  const [expanded, setExpanded] = useState(false)
  const summary = headerSummary(value)
  const count = childCount(value)

  return (
    <>
      <div className="dc-row dc-row-foldout" data-depth={depth} onClick={() => setExpanded(e => !e)}>
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
              <FieldRow key={fc.name + i} label={fc.name} value={fc.value} depth={depth + 1} />
            ))}
          {value.kind === 'Array' &&
            value.items.map((item, i) => (
              <FieldRow key={i} label={`Element ${i}`} value={item} depth={depth + 1} />
            ))}
          {value.kind === 'Dict' &&
            value.entries.map((e, i) => (
              <FieldRow key={i} label={dictKeyText(e.key)} value={e.value} depth={depth + 1} />
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

// ─── Node mode (GraphView) ───────────────────────────────────────────────
// Reuses the expanded inspector so users can drill into nested data.
// Collapses by default when there are too many fields to keep nodes compact.

const NODE_PEEK_FIELDS = 4

export function DataCardNode({
  fields,
  showAll,
  onToggle,
}: {
  fields: FieldCell[]
  showAll: boolean
  onToggle: () => void
}) {
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return (
    <div className="dc-node-card">
      <DataCardExpanded fields={visible} />
      {fields.length > NODE_PEEK_FIELDS && (
        <button className="dc-node-more" onClick={onToggle}>
          {showAll ? '收起' : `显示全部 (+${fields.length - NODE_PEEK_FIELDS})`}
        </button>
      )}
    </div>
  )
}
