import {
  useState,
  useEffect,
  useRef,
  useContext,
  createContext,
  useMemo,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type DragEvent as ReactDragEvent,
} from 'react'
import type { FieldCell } from '../bindings/FieldCell'
import type { FieldMode } from '../bindings/FieldMode'
import type { RefTarget } from '../bindings/RefTarget'
import type { SpreadInfo } from '../bindings/SpreadInfo'
import type { DictKey, FieldPathSegment, FieldValue } from '../wire'
import {
  boolValue,
  cellSpreadInfo,
  coordinateId,
  enumValue,
  fieldPathDictKey,
  fieldPathField,
  fieldPathIndex,
  floatValue,
  intValue,
  objectFields,
  refValue,
  refValueFromTarget,
  stringValue,
} from '../wire'
import { Icon } from './Icon'
import { typeColor, enumColor } from '../utils/typeColor'
import { loadEnumVariants, loadRefTargets, buildDefaultObject } from '../utils/editContext'

export function CardHeader({
  recordKey,
  actualType,
  filePath,
  onRename,
}: {
  recordKey: string
  actualType: string
  filePath?: string
  onRename?: (newKey: string) => void
}) {
  const color = typeColor(actualType)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(recordKey)
  useEffect(() => { if (!editing) setDraft(recordKey) }, [recordKey, editing])

  const commit = () => {
    const next = draft.trim()
    setEditing(false)
    if (next && next !== recordKey && onRename) onRename(next)
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
            if (e.key === 'Escape') { setEditing(false); setDraft(recordKey) }
          }}
          onClick={e => e.stopPropagation()}
        />
      ) : (
        <span
          className={`gn-key${onRename ? ' gn-key-renameable' : ''}`}
          onDoubleClick={onRename ? () => setEditing(true) : undefined}
          title={onRename ? '双击重命名' : undefined}
        >
          {recordKey}
        </span>
      )}
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
export type FieldModeIndex = { [key in string]?: { [key in string]?: FieldMode } }

function spreadHintText(info: SpreadInfo | undefined): string | undefined {
  if (!info) return undefined
  const path = info.source_field_path.length > 0
    ? `.${info.source_field_path.join('.')}`
    : ''
  return `继承自 ${info.source.actual_type}.${info.source.key}${path}\n编辑会写回来源记录`
}

function enumVariantText(value: FieldValue & { kind: 'enum' }): string {
  return value.value.variant ?? String(value.value.value)
}

function dictEnumVariantText(key: DictKey & { kind: 'enum' }): string {
  return key.value.variant ?? String(key.value.value)
}

function valueKindLabel(v: FieldValue): string {
  switch (v.kind) {
    case 'null': return 'null'
    case 'bool': return 'bool'
    case 'int': return 'int'
    case 'float': return 'float'
    case 'string': return 'string'
    case 'enum': return v.value.enum_name
    case 'object': return v.value.actual_type
    case 'ref': return v.value.target_type
    case 'array': return v.value[0] ? `${valueKindLabel(v.value[0])}[]` : '[]'
    case 'dict': return v.value[0]
      ? `{${dictKindLabel(v.value[0][0])}:${valueKindLabel(v.value[0][1])}}`
      : '{}'
  }
}

function dictKindLabel(k: DictKey): string {
  switch (k.kind) {
    case 'string': return 'string'
    case 'int': return 'int'
    case 'enum': return k.value.enum_name
  }
}

function dictKeyText(k: DictKey): string {
  switch (k.kind) {
    case 'string': return `"${k.value}"`
    case 'int': return String(k.value)
    case 'enum': return dictEnumVariantText(k)
  }
}

export function summaryOf(v: FieldValue): string {
  switch (v.kind) {
    case 'null': return '-'
    case 'bool': return v.value ? 'true' : 'false'
    case 'int': return String(v.value)
    case 'float': return String(v.value)
    case 'string': return v.value.length > 40 ? v.value.slice(0, 38) + '…' : v.value
    case 'enum': return enumVariantText(v)
    case 'ref': return v.value.target_key
    case 'object': return v.value.actual_type
    case 'array': {
      if (v.value.length === 0) return '[]'
      const allScalar = v.value.every(i =>
        i.kind === 'bool' || i.kind === 'int' || i.kind === 'float' || i.kind === 'string' || i.kind === 'enum'
      )
      if (allScalar && v.value.length <= 6) {
        const joined = v.value.map(summaryOf).join(', ')
        if (joined.length <= 60) return `[${joined}]`
      }
      return `${valueKindLabel(v.value[0])}[${v.value.length}]`
    }
    case 'dict': {
      if (v.value.length === 0) return '{}'
      const first = v.value[0]
      return `${dictKindLabel(first[0])}->${valueKindLabel(first[1])}  (${v.value.length})`
    }
  }
}

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
    if (f.value.kind === 'object') {
      count += countVisibleRows(objectFields(f.value), expandedPaths, path)
    } else if (f.value.kind === 'array') {
      count += f.value.value.length
    } else if (f.value.kind === 'dict') {
      count += f.value.value.length
    }
  }
  return count
}

export function DataCardCompact({ value }: { value: FieldValue }) {
  return <ValueChip value={value} />
}

function ValueChip({ value }: { value: FieldValue }) {
  switch (value.kind) {
    case 'null':
      return <span className="vc vc-null">null</span>
    case 'bool':
      return (
        <span className={`vc vc-bool${value.value ? ' on' : ''}`}>
          <input type="checkbox" className="dc-checkbox dc-checkbox-ro" checked={value.value} readOnly tabIndex={-1} />
        </span>
      )
    case 'int':
    case 'float':
      return <span className="vc vc-num">{String(value.value)}</span>
    case 'string':
      return <span className="vc vc-str">{summaryOf(value)}</span>
    case 'enum':
      return (
        <span className="vc vc-enum">
          <span className="vc-enum-dot" />
          {enumVariantText(value)}
        </span>
      )
    case 'ref':
      return (
        <span className="vc vc-ref" title={`${value.value.target_type}.${value.value.target_key}`}>
          <Icon name="dot" size={9} />
          <span className="vc-ref-key">{value.value.target_key}</span>
          <span className="vc-ref-type">({value.value.target_type})</span>
        </span>
      )
    case 'object':
      return <span className="vc vc-obj">{value.value.actual_type}</span>
    case 'array':
      return <span className="vc vc-arr">{summaryOf(value)}</span>
    case 'dict':
      return <span className="vc vc-dict">{summaryOf(value)}</span>
  }
}

export interface FieldDiagnostic {
  severity: 'error' | 'warning' | 'info'
  fieldPath: string
  message: string
}

interface DiagCtxValue {
  byPath: Map<string, FieldDiagnostic[]>
  prefixes: Map<string, 'error' | 'warning'>
}
const DiagCtx = createContext<DiagCtxValue | null>(null)

function severityRank(s: 'error' | 'warning' | 'info'): number {
  return s === 'error' ? 3 : s === 'warning' ? 2 : 1
}

function strongest(a: FieldDiagnostic[]): 'error' | 'warning' | 'info' {
  let best: 'error' | 'warning' | 'info' = 'info'
  for (const d of a) if (severityRank(d.severity) > severityRank(best)) best = d.severity
  return best
}

function buildDiagCtx(diags: FieldDiagnostic[] | undefined): DiagCtxValue | null {
  if (!diags || diags.length === 0) return null
  const byPath = new Map<string, FieldDiagnostic[]>()
  const prefixes = new Map<string, 'error' | 'warning'>()
  for (const d of diags) {
    const list = byPath.get(d.fieldPath) ?? []
    list.push(d)
    byPath.set(d.fieldPath, list)
    if (d.severity === 'info') continue
    let p = d.fieldPath
    while (true) {
      const lastDot = p.lastIndexOf('.')
      const lastBracket = p.lastIndexOf('[')
      const cut = Math.max(lastDot, lastBracket)
      if (cut <= 0) break
      p = p.slice(0, cut)
      const cur = prefixes.get(p)
      if (cur === 'error') break
      if (d.severity === 'error' || cur !== 'warning') prefixes.set(p, d.severity)
    }
  }
  return { byPath, prefixes }
}

export interface ExpandedProps {
  fields: FieldCell[]
  actualType?: string
  fieldModes?: FieldModeIndex
  depth?: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  pathPrefix?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  diagnostics?: FieldDiagnostic[]
  highlightField?: string | null
  onHighlightConsumed?: () => void
}

export function DataCardExpanded({
  fields,
  actualType,
  fieldModes,
  depth = 0,
  onEdit,
  pathPrefix,
  onRowToggle,
  diagnostics,
  highlightField,
  onHighlightConsumed,
}: ExpandedProps) {
  const ctx = useMemo(() => buildDiagCtx(diagnostics), [diagnostics])
  const inspectorRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!highlightField) return
    const root = inspectorRef.current
    if (!root) return
    const exact = root.querySelector<HTMLElement>(
      `.dc-row[data-field-path="${CSS.escape(highlightField)}"]`,
    )
    const top = highlightField.match(/^[^.[]+/)?.[0]
    const fallback = top
      ? root.querySelector<HTMLElement>(`.dc-row[data-field-name="${CSS.escape(top)}"]`)
      : null
    const target = exact ?? fallback
    if (target) {
      target.scrollIntoView({ block: 'center', behavior: 'smooth' })
      target.classList.add('dc-row-flash')
      const t = setTimeout(() => target.classList.remove('dc-row-flash'), 1600)
      onHighlightConsumed?.()
      return () => clearTimeout(t)
    }
    onHighlightConsumed?.()
  }, [highlightField, onHighlightConsumed])

  const body = (
    <div className="dc-inspector" ref={inspectorRef} style={{ '--depth': depth } as CSSProperties}>
      {fields.map((fc) => {
        const fieldEdit = isDimensionDefaultField(actualType, fc.name) ? undefined : onEdit
        const spreadInfo = cellSpreadInfo(fc)
        const fieldMode = fc.annotation?.field_mode ?? fieldModes?.[actualType ?? '']?.[fc.name] ?? null
        return (
          <FieldRow
            key={fc.name}
            label={fc.name}
            value={fc.value}
            depth={depth}
            onEdit={fieldEdit}
            isSpread={!!spreadInfo}
            spreadInfo={spreadInfo}
            fieldMode={fieldMode}
            fieldModes={fieldModes}
            fieldPath={[fieldPathField(fc.name)]}
            pathKey={pathPrefix ? `${pathPrefix}.${fc.name}` : fc.name}
            onRowToggle={onRowToggle}
          />
        )
      })}
    </div>
  )
  return ctx ? <DiagCtx.Provider value={ctx}>{body}</DiagCtx.Provider> : body
}

function isDimensionDefaultField(actualType: string | undefined, fieldName: string): boolean {
  return !!actualType && actualType.endsWith('Variants') && fieldName === 'default'
}

function rowDiagSeverity(pathKey: string | undefined): { sev: 'error' | 'warning' | 'info' | null; messages: string[] } {
  const ctx = useContext(DiagCtx)
  if (!ctx || !pathKey) return { sev: null, messages: [] }
  const exact = ctx.byPath.get(pathKey)
  const prefix = ctx.prefixes.get(pathKey)
  if (!exact && !prefix) return { sev: null, messages: [] }
  const sevs: ('error' | 'warning' | 'info')[] = []
  if (exact) sevs.push(strongest(exact))
  if (prefix) sevs.push(prefix)
  let sev: 'error' | 'warning' | 'info' = 'info'
  for (const s of sevs) if (severityRank(s) > severityRank(sev)) sev = s
  return { sev, messages: exact ? exact.map(d => d.message) : [] }
}

function FieldRow({
  label,
  value,
  depth,
  onEdit,
  isSpread,
  spreadInfo,
  fieldMode,
  fieldModes,
  fieldPath,
  pathKey,
  onRowToggle,
  leading,
  trailing,
  dragProps,
}: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  isSpread?: boolean
  spreadInfo?: SpreadInfo
  fieldMode?: FieldMode | null
  fieldModes?: FieldModeIndex
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const isComplex = value.kind === 'object' || value.kind === 'array' || value.kind === 'dict'
  const canExpand = isComplex && depth < MAX_DEPTH

  if (canExpand) {
    return (
      <ExpandableRow
        label={label}
        value={value}
        depth={depth}
        onEdit={onEdit}
        isSpread={isSpread}
        spreadInfo={spreadInfo}
        fieldMode={fieldMode}
        fieldModes={fieldModes}
        fieldPath={fieldPath}
        pathKey={pathKey}
        onRowToggle={onRowToggle}
        leading={leading}
        trailing={trailing}
        dragProps={dragProps}
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
      spreadInfo={spreadInfo}
      fieldMode={fieldMode}
      pathKey={pathKey}
      leading={leading}
      trailing={trailing}
      dragProps={dragProps}
    />
  )
}

function ScalarFieldRow({
  label,
  value,
  depth,
  onCommit,
  isSpread,
  spreadInfo,
  fieldMode,
  pathKey,
  leading,
  trailing,
  dragProps,
}: {
  label: string
  value: FieldValue
  depth: number
  onCommit?: (newValue: FieldValue) => void
  isSpread?: boolean
  spreadInfo?: SpreadInfo
  fieldMode?: FieldMode | null
  pathKey?: string
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const isScalar = value.kind === 'bool' || value.kind === 'int' || value.kind === 'float'
    || value.kind === 'string' || value.kind === 'enum' || value.kind === 'ref'
  const canEdit = isScalar && !!onCommit
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = spreadHint || (diag.messages.join('\n') || undefined)

  return (
    <div className={`dc-row${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ' dc-row-diag dc-row-diag-' + diag.sev : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} title={rowTitle} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 12 }}>
        {leading}
        <span className="dc-row-label-text">{label}</span>
      </div>
      <div className="dc-row-value">
        <div className="dc-row-value-inner">
          {canEdit ? (
            <DirectEditor value={value} onCommit={onCommit!} fieldMode={fieldMode} />
          ) : (
            <ValueChip value={value} />
          )}
        </div>
        {onCommit && value.kind === 'ref' && fieldMode !== 'ref' && (
          <button
            className="btn-tiny dc-row-mode-btn"
            title={`将引用 "${value.value.target_key}" 转为内联对象（使用 schema 默认值）`}
            onClick={async e => {
              e.stopPropagation()
              const obj = await buildDefaultObject(value.value.target_type)
              if (obj) onCommit(obj)
            }}
          >内联</button>
        )}
        {trailing}
      </div>
    </div>
  )
}

function DirectEditor({
  value,
  onCommit,
  fieldMode,
}: {
  value: FieldValue
  onCommit: (next: FieldValue) => void
  fieldMode?: FieldMode | null
}) {
  if (value.kind === 'bool') {
    return (
      <input
        type="checkbox"
        className="dc-checkbox"
        checked={value.value}
        onChange={e => onCommit(boolValue(e.target.checked))}
      />
    )
  }
  if (value.kind === 'enum') {
    return <EnumDirectSelect value={value} onCommit={onCommit} />
  }
  if (value.kind === 'ref') {
    if (fieldMode === 'inline') return <ValueChip value={value} />
    return <RefDirectSelect value={value} onCommit={onCommit} />
  }
  if (value.kind === 'int' || value.kind === 'float' || value.kind === 'string') {
    return <TextDirectInput value={value} onCommit={onCommit} />
  }
  return <ValueChip value={value} />
}

function TextDirectInput({
  value,
  onCommit,
}: {
  value: FieldValue & { kind: 'int' | 'float' | 'string' }
  onCommit: (next: FieldValue) => void
}) {
  const initial = plainText(value)
  const [text, setText] = useState(initial)
  useEffect(() => { setText(initial) }, [initial])

  function commit() {
    if (text === initial) return
    const next = buildFieldValue(value, text)
    if (next) onCommit(next)
    else setText(initial)
  }

  if (value.kind === 'string') {
    return (
      <textarea
        className="dc-input dc-input-flat dc-input-textarea"
        value={text}
        rows={1}
        onChange={e => {
          setText(e.target.value)
          const el = e.target as HTMLTextAreaElement
          el.style.height = 'auto'
          el.style.height = el.scrollHeight + 'px'
        }}
        onBlur={commit}
        onKeyDown={e => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            (e.target as HTMLTextAreaElement).blur()
          }
          if (e.key === 'Escape') { setText(initial); (e.target as HTMLTextAreaElement).blur() }
        }}
      />
    )
  }

  return (
    <input
      className="dc-input dc-input-flat"
      type={value.kind === 'int' || value.kind === 'float' ? 'number' : 'text'}
      value={text}
      onChange={e => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={e => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
        if (e.key === 'Escape') { setText(initial); (e.target as HTMLInputElement).blur() }
      }}
    />
  )
}

export function EnumDirectSelect({
  value,
  onCommit,
}: {
  value: FieldValue & { kind: 'enum' }
  onCommit: (next: FieldValue) => void
}) {
  const [variants, setVariants] = useState<string[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const current = enumVariantText(value)
  const color = enumColor(value.value.enum_name)
  useEffect(() => {
    let alive = true
    setLoadError(null)
    loadEnumVariants(value.value.enum_name).then(r => {
      if (!alive) return
      if (r.ok) setVariants(r.variants)
      else { setVariants([]); setLoadError(r.error) }
    })
    return () => { alive = false }
  }, [value.value.enum_name])

  if (variants === null || variants.length === 0) {
    return (
      <span className="dc-input-wrap">
        <input
          className="dc-input dc-input-flat dc-input-enum"
          style={{ '--enum-color': color } as React.CSSProperties}
          defaultValue={current}
          aria-invalid={!!loadError}
          onBlur={e => {
            if (e.target.value !== current) {
              onCommit(enumValue(value.value.enum_name, e.target.value, value.value.value))
            }
          }}
        />
        {loadError && <span className="dc-load-error" title={loadError}>!</span>}
      </span>
    )
  }
  return (
    <select
      className="dc-input dc-input-flat dc-input-enum"
      style={{ '--enum-color': color } as React.CSSProperties}
      value={current}
      onChange={e => onCommit(enumValue(value.value.enum_name, e.target.value, value.value.value))}
    >
      {!variants.includes(current) && <option value={current}>{current}</option>}
      {variants.map(v => <option key={v} value={v}>{v}</option>)}
    </select>
  )
}

export function RefDirectSelect({
  value,
  onCommit,
  autoFocus = false,
  flat = false,
}: {
  value: FieldValue & { kind: 'ref' }
  onCommit: (next: FieldValue) => void
  autoFocus?: boolean
  /** flat=true: skip the pill wrapper, render just the select (for table cells) */
  flat?: boolean
}) {
  const [targets, setTargets] = useState<RefTarget[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    setLoadError(null)
    loadRefTargets(value.value.target_type).then(r => {
      if (!alive) return
      if (r.ok) setTargets(r.targets)
      else { setTargets([]); setLoadError(r.error) }
    })
    return () => { alive = false }
  }, [value.value.target_type])

  function commit(key: string) {
    if (key !== value.value.target_key) {
      onCommit(refValue(value.value.target_type, key))
    }
  }

  if (targets === null || targets.length === 0) {
    const input = (
      <input
        className={`dc-input dc-input-flat dc-input-ref-select${flat ? ' dc-input-ref-select-flat' : ''}`}
        defaultValue={value.value.target_key}
        autoFocus={autoFocus}
        aria-invalid={!!loadError}
        placeholder={targets === null ? '加载中...' : 'key'}
        title={value.value.target_type}
        onBlur={e => commit(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          if (e.key === 'Escape') (e.target as HTMLInputElement).blur()
        }}
      />
    )
    if (flat) return input
    return (
      <span className="dc-input-ref" title={value.value.target_type}>
        <span className="dc-input-ref-dot" />
        {input}
        {loadError && <span className="dc-load-error" title={loadError}>!</span>}
      </span>
    )
  }

  const currentId = value.value.target_key
    ? coordinateId({ actual_type: value.value.target_type, key: value.value.target_key })
    : ''
  const targetById = new Map(targets.map(t => [refTargetId(t), t]))
  const inList = currentId ? targetById.has(currentId) : false

  const select = (
    <select
      className={`dc-input dc-input-flat dc-input-ref-select${flat ? ' dc-input-ref-select-flat' : ''}`}
      value={currentId}
      autoFocus={autoFocus}
      onChange={e => {
        const target = targetById.get(e.target.value)
        if (target) onCommit(refValueFromTarget(target))
      }}
      title={value.value.target_type}
    >
      {!value.value.target_key && <option value="" disabled>选择...</option>}
      {value.value.target_key && !inList && <option value={currentId}>{value.value.target_key}</option>}
      {targets.map(t => <option key={refTargetId(t)} value={refTargetId(t)}>{refTargetLabel(t)}</option>)}
    </select>
  )

  if (flat) return select
  return (
    <span className="dc-input-ref">
      <span className="dc-input-ref-dot" />
      {select}
    </span>
  )
}

function refTargetId(target: RefTarget): string {
  return coordinateId(target.coordinate)
}

function refTargetLabel(target: RefTarget): string {
  return target.coordinate.key
}

export function InlineEditor({
  value,
  onCommit,
  onCancel,
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

  if (value.kind === 'bool') {
    return (
      <input
        type="checkbox"
        className="dc-checkbox"
        checked={editVal === 'true'}
        autoFocus
        onChange={e => {
          const next = e.target.checked ? 'true' : 'false'
          setEditVal(next)
          commit(next)
        }}
        onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
      />
    )
  }
  if (value.kind === 'enum') {
    return (
      <EnumSelect
        value={value}
        current={editVal}
        onCommit={variant => onCommit(enumValue(value.value.enum_name, variant, value.value.value))}
        onCancel={onCancel}
      />
    )
  }
  if (value.kind === 'ref') {
    return <RefSelect value={value} onCommit={onCommit} onCancel={onCancel} />
  }
  if (value.kind === 'string') {
    return (
      <textarea
        className="dc-input dc-input-textarea"
        value={editVal}
        autoFocus
        rows={1}
        onChange={e => {
          setEditVal(e.target.value)
          const el = e.target as HTMLTextAreaElement
          el.style.height = 'auto'
          el.style.height = el.scrollHeight + 'px'
        }}
        onBlur={() => commit(editVal)}
        onKeyDown={e => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            commit(editVal)
          }
          if (e.key === 'Escape') onCancel()
        }}
      />
    )
  }
  return (
    <input
      className="dc-input"
      type={value.kind === 'int' || value.kind === 'float' ? 'number' : 'text'}
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
  value,
  current,
  onCommit,
  onCancel,
}: {
  value: FieldValue & { kind: 'enum' }
  current: string
  onCommit: (v: string) => void
  onCancel: () => void
}) {
  const [variants, setVariants] = useState<string[] | null>(null)
  useEffect(() => {
    let alive = true
    loadEnumVariants(value.value.enum_name).then(r => { if (alive) setVariants(r.ok ? r.variants : []) })
    return () => { alive = false }
  }, [value.value.enum_name])

  if (variants === null) {
    return <input className="dc-input" value={current} disabled placeholder="加载中..." />
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
      onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
    >
      {!variants.includes(current) && <option value={current}>{current}</option>}
      {variants.map(v => <option key={v} value={v}>{v}</option>)}
    </select>
  )
}

function RefSelect({
  value,
  onCommit,
  onCancel,
}: {
  value: FieldValue & { kind: 'ref' }
  onCommit: (next: FieldValue) => void
  onCancel: () => void
}) {
  const [targets, setTargets] = useState<RefTarget[] | null>(null)
  const [val, setVal] = useState(value.value.target_key)
  useEffect(() => {
    let alive = true
    loadRefTargets(value.value.target_type).then(r => { if (alive) setTargets(r.ok ? r.targets : []) })
    return () => { alive = false }
  }, [value.value.target_type])
  useEffect(() => { setVal(value.value.target_key) }, [value.value.target_key])

  if (targets && targets.length > 0) {
    const currentId = value.value.target_key
      ? coordinateId({ actual_type: value.value.target_type, key: value.value.target_key })
      : ''
    const targetById = new Map(targets.map(t => [refTargetId(t), t]))
    const inList = currentId ? targetById.has(currentId) : false
    return (
      <select
        className="dc-input dc-input-ref-select"
        value={currentId}
        autoFocus
        title={value.value.target_type}
        onChange={e => {
          const target = targetById.get(e.target.value)
          if (target) onCommit(refValueFromTarget(target))
        }}
        onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
      >
        {!value.value.target_key && <option value="" disabled>选择...</option>}
        {value.value.target_key && !inList && <option value={currentId}>{value.value.target_key}</option>}
        {targets.map(t => <option key={refTargetId(t)} value={refTargetId(t)}>{refTargetLabel(t)}</option>)}
      </select>
    )
  }

  return (
    <input
      className="dc-input dc-input-ref-select"
      value={val}
      autoFocus
      title={value.value.target_type}
      placeholder={targets === null ? '加载中...' : 'key'}
      onChange={e => setVal(e.target.value)}
      onBlur={() => onCommit(refValue(value.value.target_type, val))}
      onKeyDown={e => {
        if (e.key === 'Enter') onCommit(refValue(value.value.target_type, val))
        if (e.key === 'Escape') onCancel()
      }}
    />
  )
}

function ExpandableRow({
  label,
  value,
  depth,
  onEdit,
  isSpread,
  spreadInfo,
  fieldMode,
  fieldModes,
  fieldPath,
  pathKey,
  onRowToggle,
  leading,
  trailing,
  dragProps,
}: {
  label: string
  value: FieldValue
  depth: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  isSpread?: boolean
  spreadInfo?: SpreadInfo
  fieldMode?: FieldMode | null
  fieldModes?: FieldModeIndex
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const [expanded, setExpanded] = useState(false)
  const [pickingRef, setPickingRef] = useState(false)
  const summary = headerSummary(value)
  const count = childCount(value)
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = spreadHint || (diag.messages.join('\n') || undefined)

  function toggle() {
    const next = !expanded
    setExpanded(next)
    if (pathKey) onRowToggle?.(pathKey, next)
  }

  return (
    <>
      <div className={`dc-row dc-row-foldout${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ' dc-row-diag dc-row-diag-' + diag.sev : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} title={rowTitle} onClick={toggle} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
        <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 4 }}>
          {leading}
          <span className="dc-fold-arrow">
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
          </span>
          <span className="dc-row-label-text">{label}</span>
        </div>
        <div className="dc-row-value">
          {pickingRef && value.kind === 'object' ? (
            <span className="dc-row-value-inner" onClick={e => e.stopPropagation()}>
              <RefDirectSelect
                value={refValue(value.value.actual_type, '') as FieldValue & { kind: 'ref' }}
                autoFocus
                onCommit={next => {
                  setPickingRef(false)
                  if (next.kind !== 'ref' || !next.value.target_key) return
                  onEdit?.(fieldPath, next)
                }}
              />
              <button className="btn-tiny" onClick={e => { e.stopPropagation(); setPickingRef(false) }}>x</button>
            </span>
          ) : (
            <div className="dc-row-value-inner">
              <span className="vc vc-type">{summary}</span>
              {count !== null && <span className="vc-count">{count}</span>}
            </div>
          )}
          {onEdit && value.kind === 'object' && !pickingRef && fieldMode !== 'inline' && (
            <button
              className="btn-tiny dc-row-mode-btn"
              title="切换为引用（指向已有同类型记录）"
              onClick={e => { e.stopPropagation(); setPickingRef(true) }}
            >-&gt;Ref</button>
          )}
          {trailing}
        </div>
      </div>
      {expanded && (
        <>
          {value.kind === 'object' &&
            objectFields(value).map((fc) => {
                const childMode = fc.annotation?.field_mode
                  ?? fieldModes?.[value.value.actual_type]?.[fc.name]
                  ?? null
                return (
              <FieldRow
                key={fc.name}
                label={fc.name}
                value={fc.value}
                depth={depth + 1}
                onEdit={onEdit}
                fieldMode={childMode}
                fieldModes={fieldModes}
                fieldPath={[...fieldPath, fieldPathField(fc.name)]}
                pathKey={pathKey ? `${pathKey}.${fc.name}` : fc.name}
                onRowToggle={onRowToggle}
              />
                )
              })}
          {value.kind === 'array' && (
            <ArrayItems
              container={value}
              depth={depth + 1}
              fieldMode={fieldMode}
              fieldPath={fieldPath}
              pathKey={pathKey}
              onEdit={onEdit}
              fieldModes={fieldModes}
              onRowToggle={onRowToggle}
            />
          )}
          {value.kind === 'dict' &&
            value.value.map(([key, item]) => (
              <FieldRow
                key={dictKeyText(key)}
                label={dictKeyText(key)}
                value={item}
                depth={depth + 1}
                onEdit={onEdit}
                fieldMode={fieldMode}
                fieldModes={fieldModes}
                fieldPath={[...fieldPath, fieldPathDictKey(dictKeyPathText(key))]}
                pathKey={pathKey ? `${pathKey}[${dictKeyText(key)}]` : `[${dictKeyText(key)}]`}
                onRowToggle={onRowToggle}
                trailing={onEdit ? (
                  <DeleteButton
                    title="删除"
                    onClick={() => onEdit(fieldPath, dictRemove(value, key))}
                  />
                ) : undefined}
              />
            ))}
          {onEdit && (value.kind === 'array' || value.kind === 'dict') && (
            <CollectionAddRow
              container={value}
              depth={depth + 1}
              onAdd={next => onEdit(fieldPath, next)}
            />
          )}
          {value.kind === 'array' && value.value.length === 0 && (
            <EmptyHint depth={depth + 1} text="空数组" />
          )}
          {value.kind === 'dict' && value.value.length === 0 && (
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
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 12 }} />
      <div className="dc-row-value">
        <span className="vc vc-null">{text}</span>
      </div>
    </div>
  )
}

function headerSummary(v: FieldValue): string {
  switch (v.kind) {
    case 'object': return v.value.actual_type
    case 'array': return v.value[0] ? `${valueKindLabel(v.value[0])}[]` : 'array'
    case 'dict': return v.value[0]
      ? `${dictKindLabel(v.value[0][0])} -> ${valueKindLabel(v.value[0][1])}`
      : 'dict'
    default: return ''
  }
}

function childCount(v: FieldValue): number | null {
  switch (v.kind) {
    case 'array': return v.value.length
    case 'dict': return v.value.length
    default: return null
  }
}

function arrayMove(arr: FieldValue & { kind: 'array' }, from: number, to: number): FieldValue {
  if (from === to || from < 0 || to < 0 || from >= arr.value.length || to >= arr.value.length) {
    return arr
  }
  const items = arr.value.slice()
  const [moved] = items.splice(from, 1)
  items.splice(to, 0, moved)
  return { kind: 'array', value: items }
}

function arrayRemove(arr: FieldValue & { kind: 'array' }, i: number): FieldValue {
  const items = arr.value.slice()
  items.splice(i, 1)
  return { kind: 'array', value: items }
}

function arrayAppend(arr: FieldValue & { kind: 'array' }, value: FieldValue): FieldValue {
  return { kind: 'array', value: [...arr.value, value] }
}

function dictRemove(d: FieldValue & { kind: 'dict' }, key: DictKey): FieldValue {
  return { kind: 'dict', value: d.value.filter(([entryKey]) => !dictKeyEq(entryKey, key)) }
}

function dictInsert(d: FieldValue & { kind: 'dict' }, key: DictKey, value: FieldValue): FieldValue {
  const idx = d.value.findIndex(([entryKey]) => dictKeyEq(entryKey, key))
  if (idx >= 0) {
    const entries = d.value.slice()
    entries[idx] = [key, value]
    return { kind: 'dict', value: entries }
  }
  return { kind: 'dict', value: [...d.value, [key, value]] }
}

function dictKeyEq(a: DictKey, b: DictKey): boolean {
  if (a.kind !== b.kind) return false
  if (a.kind === 'string' && b.kind === 'string') return a.value === b.value
  if (a.kind === 'int' && b.kind === 'int') return a.value === b.value
  if (a.kind === 'enum' && b.kind === 'enum') {
    return a.value.enum_name === b.value.enum_name && a.value.variant === b.value.variant && a.value.value === b.value.value
  }
  return false
}

function dictKeyPathText(key: DictKey): string {
  switch (key.kind) {
    case 'string': return JSON.stringify(key.value)
    case 'int': return String(key.value)
    case 'enum': {
      const variant = key.value.variant
      return variant ? `${key.value.enum_name}.${variant}` : `${key.value.enum_name}(${key.value.value})`
    }
  }
}

function replaceValueAtPath(
  root: FieldValue,
  path: FieldPathSegment[],
  replacement: FieldValue,
): FieldValue | null {
  if (path.length === 0) return replacement
  const [head, ...tail] = path
  if (head.kind === 'field') {
    if (root.kind !== 'object') return null
    const current = root.value.fields[head.value]
    if (!current) return null
    const next = replaceValueAtPath(current, tail, replacement)
    if (!next) return null
    return {
      kind: 'object',
      value: {
        ...root.value,
        fields: {
          ...root.value.fields,
          [head.value]: next,
        },
      },
    }
  }
  if (head.kind === 'index') {
    if (root.kind !== 'array') return null
    const current = root.value[head.value]
    if (!current) return null
    const next = replaceValueAtPath(current, tail, replacement)
    if (!next) return null
    const values = root.value.slice()
    values[head.value] = next
    return { kind: 'array', value: values }
  }
  return null
}

function defaultElementFor(container: FieldValue): FieldValue {
  if (container.kind === 'array') {
    const sample = container.value.find(i => i.kind !== 'null') ?? container.value[0]
    if (sample) return defaultLikeShape(sample)
  }
  if (container.kind === 'dict') {
    const sample = container.value.find(([, item]) => item.kind !== 'null')?.[1]
      ?? container.value[0]?.[1]
    if (sample) return defaultLikeShape(sample)
  }
  return stringValue('')
}

function defaultLikeShape(sample: FieldValue): FieldValue {
  switch (sample.kind) {
    case 'bool': return boolValue(false)
    case 'int': return intValue(0)
    case 'float': return floatValue(0)
    case 'string': return stringValue('')
    case 'null': return stringValue('')
    case 'enum': return enumValue(sample.value.enum_name, sample.value.variant, sample.value.value)
    case 'ref': return refValue(sample.value.target_type, '')
    case 'object': return {
      kind: 'object',
      value: {
        key: '',
        actual_type: sample.value.actual_type,
        fields: Object.fromEntries(objectFields(sample).map(f => [f.name, defaultLikeShape(f.value)])),
      },
    }
    case 'array': return { kind: 'array', value: [] }
    case 'dict': return { kind: 'dict', value: [] }
  }
}

function ArrayItems({
  container,
  depth,
  fieldMode,
  fieldPath,
  pathKey,
  onEdit,
  onRowToggle,
  fieldModes,
}: {
  container: FieldValue & { kind: 'array' }
  depth: number
  fieldMode?: FieldMode | null
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onRowToggle?: (path: string, expanded: boolean) => void
  fieldModes?: FieldModeIndex
}) {
  const [dragIdx, setDragIdx] = useState<number | null>(null)
  const [overIdx, setOverIdx] = useState<number | null>(null)
  const dragArmedRef = useRef<number | null>(null)

  function dropAt(target: number) {
    if (dragIdx === null || dragIdx === target) return
    onEdit?.(fieldPath, arrayMove(container, dragIdx, target))
    setDragIdx(null)
    setOverIdx(null)
  }

  return (
    <>
      {container.value.map((item, i) => {
        const dragHandle = onEdit ? <DragHandle rowIndex={i} dragArmedRef={dragArmedRef} /> : undefined
        const trailing = onEdit ? (
          <DeleteButton title="删除" onClick={() => onEdit(fieldPath, arrayRemove(container, i))} />
        ) : undefined
        return (
          <FieldRow
            key={i}
            label={`[${i}]`}
            value={item}
            depth={depth}
            onEdit={onEdit}
            fieldMode={fieldMode}
            fieldModes={fieldModes}
            fieldPath={[...fieldPath, fieldPathIndex(i)]}
            pathKey={pathKey ? `${pathKey}[${i}]` : `[${i}]`}
            onRowToggle={onRowToggle}
            leading={dragHandle}
            trailing={trailing}
            dragProps={onEdit ? {
              extraClass: `dc-row-draggable${overIdx === i && dragIdx !== null && dragIdx !== i ? ' drop-target' : ''}${dragIdx === i ? ' dragging' : ''}`,
              draggable: true,
              onDragStart: (e: ReactDragEvent) => {
                if (dragArmedRef.current !== i) {
                  e.preventDefault()
                  return
                }
                e.dataTransfer.effectAllowed = 'move'
                e.dataTransfer.setData('text/plain', String(i))
                setDragIdx(i)
              },
              onDragOver: (e: ReactDragEvent) => {
                if (dragIdx === null) return
                e.preventDefault()
                e.dataTransfer.dropEffect = 'move'
                if (overIdx !== i) setOverIdx(i)
              },
              onDragLeave: () => { if (overIdx === i) setOverIdx(null) },
              onDrop: (e: ReactDragEvent) => { e.preventDefault(); dropAt(i) },
              onDragEnd: () => {
                dragArmedRef.current = null
                setDragIdx(null); setOverIdx(null)
              },
            } : undefined}
          />
        )
      })}
    </>
  )
}

function DragHandle({ rowIndex, dragArmedRef }: {
  rowIndex: number
  dragArmedRef: React.MutableRefObject<number | null>
}) {
  return (
    <span
      className="dc-drag-handle"
      title="拖动重排"
      onMouseDown={() => { dragArmedRef.current = rowIndex }}
      onMouseUp={() => { dragArmedRef.current = null }}
      onClick={e => e.stopPropagation()}
    >
      <svg width="8" height="14" viewBox="0 0 8 14" fill="currentColor" aria-hidden>
        <circle cx="2" cy="3" r="1" /><circle cx="6" cy="3" r="1" />
        <circle cx="2" cy="7" r="1" /><circle cx="6" cy="7" r="1" />
        <circle cx="2" cy="11" r="1" /><circle cx="6" cy="11" r="1" />
      </svg>
    </span>
  )
}

function DeleteButton({ onClick, title }: { onClick: () => void; title: string }) {
  return (
    <button
      className="btn-tiny btn-tiny-danger dc-row-delete"
      title={title}
      onClick={(e: ReactMouseEvent) => { e.stopPropagation(); onClick() }}
    >x</button>
  )
}

function CollectionAddRow({ container, depth, onAdd }: {
  container: FieldValue & { kind: 'array' | 'dict' }
  depth: number
  onAdd: (next: FieldValue) => void
}) {
  const [adding, setAdding] = useState(false)
  const [dupError, setDupError] = useState<string | null>(null)

  function reset() { setAdding(false); setDupError(null) }

  if (container.kind === 'array') {
    const sample = container.value[0]
    const needsPicker = sample && (sample.kind === 'enum' || sample.kind === 'ref' || sample.kind === 'bool')
    if (!needsPicker) {
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
            <button className="btn-tiny" onClick={reset}>x</button>
          </span>
        )}
      </div>
    )
  }

  const sampleKey: DictKey = container.value[0]?.[0] ?? { kind: 'string', value: '' }
  function tryAdd(key: DictKey) {
    if (container.kind !== 'dict') return
    const dup = container.value.some(([entryKey]) => dictKeyEq(entryKey, key))
    if (dup) {
      setDupError(`键 "${dictKeyText(key)}" 已存在`)
      return
    }
    onAdd(dictInsert(container, key, defaultElementFor(container)))
    reset()
  }
  return (
    <div className="dc-row dc-row-add" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
      {!adding ? (
        <button className="btn-add-item" onClick={() => { setAdding(true); setDupError(null) }}>
          <Icon name="plus" size={11} /> 添加项
        </button>
      ) : (
        <span className="dc-add-stack">
          <DictKeyEntry
            sampleKey={sampleKey}
            onCommit={tryAdd}
            onCancel={reset}
          />
          {dupError && <span className="dc-inline-error" role="alert">{dupError}</span>}
        </span>
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
  const [variants, setVariants] = useState<string[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  useEffect(() => {
    if (sampleKey.kind !== 'enum') return
    let alive = true
    setLoadError(null)
    loadEnumVariants(sampleKey.value.enum_name).then(r => {
      if (!alive) return
      if (r.ok) setVariants(r.variants)
      else { setVariants([]); setLoadError(r.error) }
    })
    return () => { alive = false }
  }, [sampleKey.kind === 'enum' ? sampleKey.value.enum_name : ''])

  if (sampleKey.kind === 'enum') {
    if (variants === null) {
      return <span className="dc-add-form"><span className="dc-add-loading">加载枚举...</span></span>
    }
    if (variants.length === 0) {
      return (
        <span className="dc-add-form">
          {loadError && <span className="dc-load-error" title={loadError}>!</span>}
          <input
            className="dc-input" autoFocus value={text}
            placeholder="枚举变体"
            aria-invalid={!!loadError}
            onChange={e => setText(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && text) onCommit({ kind: 'enum', value: { enum_name: sampleKey.value.enum_name, variant: text, value: 0n } })
              if (e.key === 'Escape') onCancel()
            }}
          />
          <button className="btn-tiny" onClick={() => text && onCommit({ kind: 'enum', value: { enum_name: sampleKey.value.enum_name, variant: text, value: 0n } })}>✓</button>
          <button className="btn-tiny" onClick={onCancel}>x</button>
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
            if (e.target.value) onCommit({ kind: 'enum', value: { enum_name: sampleKey.value.enum_name, variant: e.target.value, value: 0n } })
          }}
          onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
        >
          <option value="" disabled>选择...</option>
          {variants.map(v => <option key={v} value={v}>{v}</option>)}
        </select>
        <button className="btn-tiny" onClick={onCancel}>x</button>
      </span>
    )
  }

  function commit() {
    if (!text) return
    if (sampleKey.kind === 'int') {
      try {
        onCommit({ kind: 'int', value: BigInt(text) })
      } catch {
        return
      }
    } else {
      onCommit({ kind: 'string', value: text })
    }
  }
  return (
    <span className="dc-add-form">
      <input
        className="dc-input"
        placeholder={sampleKey.kind === 'int' ? '整数 key' : '字符串 key'}
        autoFocus
        value={text}
        onChange={e => setText(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') commit()
          if (e.key === 'Escape') onCancel()
        }}
      />
      <button className="btn-tiny" onClick={commit}>✓</button>
      <button className="btn-tiny" onClick={onCancel}>x</button>
    </span>
  )
}

function buildFieldValue(original: FieldValue, raw: string): FieldValue | null {
  switch (original.kind) {
    case 'bool':
      return boolValue(raw === 'true')
    case 'int':
      try {
        return intValue(raw)
      } catch {
        return null
      }
    case 'float': {
      const n = parseFloat(raw)
      return Number.isFinite(n) ? floatValue(n) : null
    }
    case 'string':
      return stringValue(raw)
    case 'enum':
      return enumValue(original.value.enum_name, raw, original.value.value)
    case 'ref':
      return refValue(original.value.target_type, raw)
    default:
      return null
  }
}

function plainText(v: FieldValue): string {
  switch (v.kind) {
    case 'bool': return v.value ? 'true' : 'false'
    case 'int': return String(v.value)
    case 'float': return String(v.value)
    case 'string': return v.value
    case 'enum': return enumVariantText(v)
    case 'ref': return v.value.target_key
    default: return ''
  }
}

export function DataCardNode({
  fields,
  actualType,
  fieldModes,
  showAll,
  onToggle,
  onRowToggle,
  onEdit,
}: {
  fields: FieldCell[]
  actualType: string
  fieldModes?: FieldModeIndex
  showAll: boolean
  onToggle: () => void
  onRowToggle?: (path: string, expanded: boolean) => void
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
}) {
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return (
    <div className="dc-node-card">
      <DataCardExpanded
        fields={visible}
        actualType={actualType}
        fieldModes={fieldModes}
        onRowToggle={onRowToggle}
        onEdit={onEdit}
      />
      {fields.length > NODE_PEEK_FIELDS && (
        <button className="dc-node-more" onClick={onToggle}>
          {showAll ? '收起' : `显示全部 (+${fields.length - NODE_PEEK_FIELDS})`}
        </button>
      )}
    </div>
  )
}
