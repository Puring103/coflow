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
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { FieldDiagnostic as WireFieldDiagnostic } from '../bindings/FieldDiagnostic'
import type { SpreadInfo } from '../bindings/SpreadInfo'
import type { DictKey, FieldPathSegment, FieldValue } from '../wire'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import {
  annotationChild,
  annotationDeclaredType,
  annotationEnumType,
  annotationItem,
  annotationNullable,
  annotationPolymorphicTypes,
  annotationRefTargetType,
  boolValue,
  cellDeclaredType,
  cellEnumType,
  cellItemAnnotation,
  cellNullable,
  cellReadOnly,
  cellRefTargetType,
  cellSpreadInfo,
  enumValue,
  fieldPathDictKey,
  fieldPathField,
  fieldPathIndex,
  nullValue,
  objectFields,
  refValue,
} from '../wire'
import { Icon } from './Icon'
import { DiagBadge } from './DiagBadge'
import { typeColor, enumColor } from '../utils/typeColor'
import { useEditorLookups } from '../utils/editContext'
import type { EditorLookupAccess } from '../utils/editContext'
import {
  collectionShapeForDeclaredType,
  parseFieldValueText,
  plainFieldValueText,
  scalarDefaultForDeclaredType,
  summaryOf,
} from '../value/fieldValue'
import { useObjectDraft } from './ObjectDraftHost'
import { NODE_PEEK_FIELDS } from './DataCard.geometry'

export function CardHeader({
  recordKey,
  actualType,
  filePath,
  onRename,
  diagSeverity,
  onDiagBadgeClick,
  highlight,
}: {
  recordKey: string
  actualType: string
  filePath?: string
  onRename?: (newKey: string) => void
  /** Record-level severity: shows a corner badge that focuses the panel. */
  diagSeverity?: 'error' | 'warning' | null
  onDiagBadgeClick?: () => void
  /** When true, the header briefly pulses (record-level diagnostic jump). */
  highlight?: boolean
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
    <div className={`gn-header${highlight ? ' gn-header-flash' : ''}`} style={{ '--node-color': color } as CSSProperties}>
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
      {(diagSeverity === 'error' || diagSeverity === 'warning') && (
        <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
      )}
    </div>
  )
}

const MAX_DEPTH = 5
const INDENT_PX = 14

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
    case 'ref': return '&'
    case 'array': return v.value[0] ? `${valueKindLabel(v.value[0])}[]` : '[]'
    case 'dict': return v.value[0]
      ? `{${dictKindLabel(v.value[0][0])}:${valueKindLabel(v.value[0][1])}}`
      : '{}'
  }
}

function typeLabelForValue(v: FieldValue, declaredType?: string): string {
  return declaredType ?? valueKindLabel(v)
}

/** Strip trailing `?` off a declared type string. Kept for the rare cases
 *  (null-collection detection, resolveDefaultElement scalar shorthand) that
 *  still work on the wire-formatted type string. Other schema questions
 *  should read `FieldAnnotation.item_annotation` / `.ref_target_type` /
 *  `.enum_type` instead — the backend fills those directly. */
function stripNullableType(declaredType?: string): string | undefined {
  return declaredType?.endsWith('?') ? declaredType.slice(0, -1) : declaredType
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
        <span className="vc vc-ref" title={`&${value.value}`}>
          <Icon name="dot" size={9} />
          <span className="vc-ref-key">{value.value}</span>
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

export type FieldDiagnostic = WireFieldDiagnostic

interface DiagCtxValue {
  byPath: Map<string, FieldDiagnostic[]>
  prefixes: Map<string, 'error' | 'warning'>
  onBadgeClick?: (topLevelFieldPath: string) => void
}
const DiagCtx = createContext<DiagCtxValue | null>(null)

/** Set of pathKeys whose ExpandableRow should auto-expand on mount / when
 *  the set changes. Used when a diagnostic jump lands on a nested field so
 *  the row is actually visible after scrollIntoView. Cleared by whoever set
 *  it once the highlight has been consumed. */
const AutoExpandCtx = createContext<ReadonlySet<string>>(new Set())
const ControlledExpansionCtx = createContext<ReadonlySet<string> | null>(null)
const ValueRowSelectionCtx = createContext<{
  selectedFieldPath?: FieldPathSegment[] | null
  onSelectValue?: (fieldPath: FieldPathSegment[]) => void
  onEditingFinished?: () => void
} | null>(null)

function sameFieldPath(
  left: FieldPathSegment[] | null | undefined,
  right: FieldPathSegment[],
): boolean {
  return !!left
    && left.length === right.length
    && left.every((segment, index) => (
      segment.kind === right[index].kind && segment.value === right[index].value
    ))
}

function severityRank(s: 'error' | 'warning' | 'info'): number {
  return s === 'error' ? 3 : s === 'warning' ? 2 : 1
}

function normalizedDiagnosticSeverity(severity: string): 'error' | 'warning' | 'info' {
  return severity === 'error' || severity === 'warning' ? severity : 'info'
}

function strongest(a: FieldDiagnostic[]): 'error' | 'warning' | 'info' {
  let best: 'error' | 'warning' | 'info' = 'info'
  for (const d of a) {
    const sev = normalizedDiagnosticSeverity(d.severity)
    if (severityRank(sev) > severityRank(best)) best = sev
  }
  return best
}

function buildDiagCtx(
  diags: FieldDiagnostic[] | undefined,
  onBadgeClick?: (topLevelFieldPath: string) => void,
): DiagCtxValue | null {
  if (!diags || diags.length === 0) return null
  const byPath = new Map<string, FieldDiagnostic[]>()
  const prefixes = new Map<string, 'error' | 'warning'>()
  for (const d of diags) {
    const fieldPath = d.field_path
    const list = byPath.get(fieldPath) ?? []
    list.push(d)
    byPath.set(fieldPath, list)
    const severity = normalizedDiagnosticSeverity(d.severity)
    if (severity === 'info') continue
    let p = fieldPath
    while (true) {
      const lastDot = p.lastIndexOf('.')
      const lastBracket = p.lastIndexOf('[')
      const cut = Math.max(lastDot, lastBracket)
      if (cut <= 0) break
      p = p.slice(0, cut)
      const cur = prefixes.get(p)
      if (cur === 'error') break
      if (severity === 'error' || cur !== 'warning') prefixes.set(p, severity)
    }
  }
  return { byPath, prefixes, onBadgeClick }
}

export interface ExpandedProps {
  fields: FieldCell[]
  actualType?: string
  depth?: number
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  pathPrefix?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  expandedPaths?: ReadonlySet<string>
  diagnostics?: FieldDiagnostic[]
  highlightField?: string | null
  onHighlightConsumed?: () => void
  /** Called when the user clicks the corner badge of a diagnostic row.
   *  Argument is the top-level field name (so nested-row problems still
   *  route the panel focus to the same anchor the table cell uses). */
  onDiagnosticBadgeClick?: (topLevelFieldPath: string) => void
  /** Automatically expand every prefix of this path once, so a diagnostic
   *  jump into a deeply nested field can actually reach its target row.
   *  Cleared via `onHighlightConsumed` alongside `highlightField`. */
  expandAlongPath?: string | null
  selectedFieldPath?: FieldPathSegment[] | null
  onSelectValue?: (fieldPath: FieldPathSegment[]) => void
  onEditingFinished?: () => void
}

export function DataCardExpanded({
  fields,
  actualType,
  depth = 0,
  onEdit,
  onCollectionEdit,
  pathPrefix,
  onRowToggle,
  expandedPaths,
  diagnostics,
  highlightField,
  onHighlightConsumed,
  onDiagnosticBadgeClick,
  expandAlongPath,
  selectedFieldPath,
  onSelectValue,
  onEditingFinished,
}: ExpandedProps) {
  const ctx = useMemo(
    () => buildDiagCtx(diagnostics, onDiagnosticBadgeClick),
    [diagnostics, onDiagnosticBadgeClick],
  )
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
    // Target not yet in the DOM — auto-expand along the path is likely still
    // rendering. Defer to a microtask; if it still isn't there, retry a few
    // times before giving up so nested foldouts have a chance to mount.
    let attempts = 0
    let raf = 0
    const retry = () => {
      const nowRoot = inspectorRef.current
      if (!nowRoot) return
      const hit = nowRoot.querySelector<HTMLElement>(
        `.dc-row[data-field-path="${CSS.escape(highlightField)}"]`,
      ) ?? (top
        ? nowRoot.querySelector<HTMLElement>(`.dc-row[data-field-name="${CSS.escape(top)}"]`)
        : null)
      if (hit) {
        hit.scrollIntoView({ block: 'center', behavior: 'smooth' })
        hit.classList.add('dc-row-flash')
        setTimeout(() => hit.classList.remove('dc-row-flash'), 1600)
        onHighlightConsumed?.()
        return
      }
      if (++attempts >= 6) {
        onHighlightConsumed?.()
        return
      }
      raf = requestAnimationFrame(retry)
    }
    raf = requestAnimationFrame(retry)
    return () => cancelAnimationFrame(raf)
  }, [highlightField, onHighlightConsumed])

  const autoExpandSet = useMemo(() => {
    if (!expandAlongPath) return new Set<string>()
    const set = new Set<string>()
    let cur = expandAlongPath
    set.add(cur)
    while (true) {
      const lastDot = cur.lastIndexOf('.')
      const lastBracket = cur.lastIndexOf('[')
      const cut = Math.max(lastDot, lastBracket)
      if (cut <= 0) break
      cur = cur.slice(0, cut)
      set.add(cur)
    }
    return set
  }, [expandAlongPath])

  const body = (
    <div className="dc-inspector" ref={inspectorRef} style={{ '--depth': depth } as CSSProperties}>
      {fields.map((fc) => {
        const fieldEdit = cellReadOnly(fc) ? undefined : onEdit
        const spreadInfo = cellSpreadInfo(fc)
        const declaredType = cellDeclaredType(fc)
        const refTargetType = cellRefTargetType(fc)
        const enumType = cellEnumType(fc)
        const nullable = cellNullable(fc)
        return (
          <FieldRow
            key={fc.name}
            label={fc.name}
            value={fc.value}
            depth={depth}
            onEdit={fieldEdit}
            onCollectionEdit={fieldEdit ? onCollectionEdit : undefined}
            isSpread={!!spreadInfo}
            spreadInfo={spreadInfo}
            declaredType={declaredType}
            refTargetType={refTargetType}
            enumType={enumType}
            nullable={nullable}
            valueAnnotation={fc.annotation}
            fieldPath={[fieldPathField(fc.name)]}
            pathKey={pathPrefix ? `${pathPrefix}.${fc.name}` : fc.name}
            onRowToggle={onRowToggle}
          />
        )
      })}
    </div>
  )
  const wrapped = (
    <ValueRowSelectionCtx.Provider value={{ selectedFieldPath, onSelectValue, onEditingFinished }}>
      <ControlledExpansionCtx.Provider value={expandedPaths ?? null}>
        <AutoExpandCtx.Provider value={autoExpandSet}>{body}</AutoExpandCtx.Provider>
      </ControlledExpansionCtx.Provider>
    </ValueRowSelectionCtx.Provider>
  )
  return ctx ? <DiagCtx.Provider value={ctx}>{wrapped}</DiagCtx.Provider> : wrapped
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
  onCollectionEdit,
  isSpread,
  spreadInfo,
  declaredType,
  refTargetType,
  enumType,
  nullable,
  valueAnnotation,
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
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  isSpread?: boolean
  spreadInfo?: SpreadInfo
  declaredType?: string
  refTargetType?: string
  enumType?: string
  nullable?: boolean
  valueAnnotation?: FieldAnnotation | null
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const isComplex = value.kind === 'object' || value.kind === 'array' || value.kind === 'dict'
  // A `null` value on a field whose declared type is an array/dict/object
  // should still be treated as expandable, so the user can just click
  // "add element" instead of first coercing null → empty collection by
  // hand. The materialization happens lazily when the user hits add.
  const nullCollectionShape = value.kind === 'null' ? collectionShapeForDeclaredType(declaredType) : null
  const displayValue = nullCollectionShape ?? value
  const canExpand = (isComplex || nullCollectionShape !== null) && depth < MAX_DEPTH
  const polyTypes = annotationPolymorphicTypes(valueAnnotation)

  // Extra trailing controls for nullable / polymorphic fields. Enum and ref
  // scalars already expose a "(null)" option in their pill selects, so we
  // don't double up there. Bool doesn't get a clear button unless nullable.
  const commit = onEdit ? (next: FieldValue) => onEdit(fieldPath, next) : undefined
  const nullControls = !isSpread && commit ? (
    <NullableControls
      value={value}
      nullable={!!nullable}
      declaredType={declaredType}
      enumType={enumType}
      refTargetType={refTargetType}
      polymorphicTypes={polyTypes}
      onCommit={commit}
    />
  ) : null
  const mergedTrailing = nullControls
    ? <>{trailing}{nullControls}</>
    : trailing

  if (canExpand) {
    return (
      <ExpandableRow
        label={label}
        value={displayValue}
        depth={depth}
        onEdit={onEdit}
        onCollectionEdit={onCollectionEdit}
        isSpread={isSpread}
        spreadInfo={spreadInfo}
        declaredType={declaredType}
        refTargetType={refTargetType}
        valueAnnotation={valueAnnotation}
        fieldPath={fieldPath}
        pathKey={pathKey}
        onRowToggle={onRowToggle}
        leading={leading}
        trailing={mergedTrailing}
        dragProps={dragProps}
      />
    )
  }
  return (
    <ScalarFieldRow
      label={label}
      value={value}
      depth={depth}
      onCommit={commit}
      isSpread={isSpread}
      spreadInfo={spreadInfo}
      declaredType={declaredType}
      refTargetType={refTargetType}
      enumType={enumType}
      nullable={nullable}
      pathKey={pathKey}
      fieldPath={fieldPath}
      leading={leading}
      trailing={mergedTrailing}
      dragProps={dragProps}
    />
  )
}

function NullableControls({
  value,
  nullable,
  declaredType,
  enumType,
  refTargetType,
  polymorphicTypes,
  onCommit,
}: {
  value: FieldValue
  nullable: boolean
  declaredType?: string
  enumType?: string
  refTargetType?: string
  polymorphicTypes: string[]
  onCommit: (next: FieldValue) => void
}) {
  const isNull = value.kind === 'null'
  const isObject = value.kind === 'object'
  const isPolymorphic = polymorphicTypes.length >= 2
  const canSwitchType = isObject && isPolymorphic && !isNull
  // Clear button on any nullable, currently non-null field — including enum
  // and ref, whose own dropdowns hide the "(null)" option behind an extra
  // click. A dedicated ✕ next to the value is faster.
  const canClear = nullable && !isNull
  // Create button on any null field where we can produce something useful:
  // scalars/collections we materialize locally, refs/enums pull first option
  // via the async helper, and abstract objects prompt for a concrete type.
  const canCreate = isNull && (
    scalarDefaultForDeclaredType(declaredType) !== null
    || isPolymorphic
    || !!enumType
    || !!refTargetType
    || !!declaredType
  )

  const { openObjectDraft } = useObjectDraft()
  const lookups = useEditorLookups()

  if (!canClear && !canCreate && !canSwitchType) return null

  function openSwitchDialog() {
    if (value.kind !== 'object') return
    openObjectDraft({
      title: '切换类型',
      actualType: value.value.actual_type,
      polymorphicTypes,
      confirmLabel: '确认切换',
      onConfirm: next => onCommit(next),
    })
  }

  function openCreateDialog(chosenType: string) {
    openObjectDraft({
      title: `创建 ${chosenType}`,
      actualType: chosenType,
      polymorphicTypes: isPolymorphic ? polymorphicTypes : [],
      confirmLabel: '创建',
      onConfirm: next => onCommit(next),
    })
  }

  async function handleCreate() {
    // Scalars and collections stay local — cheap default + no user input needed.
    const scalarDefault = defaultForScalarLike({
      declaredType,
      enumType,
      refTargetType,
      lookups,
    })
    if (scalarDefault) {
      const resolved = await scalarDefault()
      if (resolved) onCommit(resolved)
      return
    }
    // Object materialization needs the draft dialog so required + abstract
    // sub-fields can be filled explicitly instead of hoping the runtime
    // hands back a writable shape.
    if (isPolymorphic) {
      // No default — user picks concrete type inside the dialog.
      openCreateDialog(polymorphicTypes[0])
      return
    }
    if (declaredType) {
      const stripped = declaredType.endsWith('?') ? declaredType.slice(0, -1) : declaredType
      openCreateDialog(stripped)
    }
  }

  return (
    <span className="dc-null-controls" onClick={e => e.stopPropagation()}>
      {canSwitchType && (
        <button
          type="button"
          className="dc-null-btn dc-null-btn-switch"
          title="切换类型"
          aria-label="切换类型"
          onClick={openSwitchDialog}
        >
          <Icon name="edit" size={11} />
        </button>
      )}
      {canClear && (
        <button
          type="button"
          className="dc-null-btn dc-null-btn-clear"
          title="清除为 null"
          aria-label="清除为 null"
          onClick={() => onCommit(nullValue())}
        >
          <Icon name="close" size={11} />
        </button>
      )}
      {canCreate && (
        <button
          type="button"
          className="dc-null-btn dc-null-btn-create"
          title="创建默认值"
          aria-label="创建默认值"
          onClick={handleCreate}
        >
          <Icon name="plus" size={11} />
        </button>
      )}
    </span>
  )
}

/** Return a synchronous or ref/enum-fetching thunk producing a starter
 *  value for scalars, refs, enums, arrays and dicts. Object types return
 *  null — those need the object-draft dialog so required and abstract
 *  sub-fields can be filled explicitly. */
function defaultForScalarLike({
  declaredType,
  enumType,
  refTargetType,
  lookups,
}: {
  declaredType?: string
  enumType?: string
  refTargetType?: string
  lookups: EditorLookupAccess
}): (() => Promise<FieldValue | null>) | null {
  if (enumType) {
    return async () => {
      const variants = await lookups.loadEnumVariants(enumType)
      if (variants.ok && variants.value.length > 0) {
        return enumValue(enumType, variants.value[0], 0n)
      }
      return null
    }
  }
  if (refTargetType) {
    return async () => {
      const targets = await lookups.loadRefTargets(refTargetType)
      if (targets.ok && targets.value.length > 0) {
        return refValue(targets.value[0].coordinate.key)
      }
      // No known ref targets — the user needs to create one first.
      alert(`&${refTargetType} 类型没有可用的记录，请先在对应的表中创建一条。`)
      return null
    }
  }
  const scalar = scalarDefaultForDeclaredType(declaredType)
  if (scalar) return async () => scalar
  return null
}

function ScalarFieldRow({
  label,
  value,
  depth,
  onCommit,
  isSpread,
  spreadInfo,
  declaredType,
  refTargetType,
  enumType,
  nullable,
  pathKey,
  fieldPath,
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
  declaredType?: string
  refTargetType?: string
  enumType?: string
  nullable?: boolean
  pathKey?: string
  fieldPath: FieldPathSegment[]
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const isScalar = value.kind === 'bool' || value.kind === 'int' || value.kind === 'float'
    || value.kind === 'string' || value.kind === 'enum' || value.kind === 'ref'
  const resolvedRefTarget = refTargetType
  const isNullDropdown = value.kind === 'null' && !!(enumType || resolvedRefTarget)
  const canEdit = (isScalar || isNullDropdown) && !!onCommit
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = spreadHint || (diag.messages.join('\n') || undefined)
  const rowSelection = useContext(ValueRowSelectionCtx)
  const selected = sameFieldPath(rowSelection?.selectedFieldPath, fieldPath)

  return (
    <div className={`dc-row${selected ? ' keyboard-selected' : ''}${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ' dc-row-diag dc-row-diag-' + diag.sev : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} data-field-path-wire={JSON.stringify(fieldPath)} title={rowTitle} onMouseDown={() => rowSelection?.onSelectValue?.(fieldPath)} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
      <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 12 }}>
        {leading}
        <span className="dc-row-label-text">{label}</span>
        {depth === 0 && declaredType && (
          <span className="dc-row-type" title={`类型：${declaredType}`}>{declaredType}</span>
        )}
      </div>
      <div className="dc-row-value">
        <div className="dc-row-value-inner">
          {canEdit ? (
            <DirectEditor value={value} onCommit={onCommit!} refTargetType={resolvedRefTarget} enumType={enumType} nullable={nullable} />
          ) : (
            <ValueChip value={value} />
          )}
        </div>
        {trailing}
      </div>
      <DiagCornerBadge severity={diag.sev} pathKey={pathKey} />
    </div>
  )
}

function DiagCornerBadge({ severity, pathKey }: {
  severity: 'error' | 'warning' | 'info' | null
  pathKey?: string
}) {
  const ctx = useContext(DiagCtx)
  if (severity !== 'error' && severity !== 'warning') return null
  const onClick = ctx?.onBadgeClick && pathKey
    ? () => ctx.onBadgeClick!(topLevelSegmentOfPathKey(pathKey))
    : undefined
  return <DiagBadge severity={severity} onClick={onClick} />
}

function topLevelSegmentOfPathKey(pathKey: string): string {
  const m = pathKey.match(/^[^.[]+/)
  return m ? m[0] : pathKey
}

export function DirectEditor({
  value,
  onCommit,
  refTargetType,
  enumType,
  nullable,
}: {
  value: FieldValue
  onCommit: (next: FieldValue) => void
  refTargetType?: string
  enumType?: string
  nullable?: boolean
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
  if (value.kind === 'enum' || (value.kind === 'null' && enumType)) {
    return <EnumDirectSelect value={value as FieldValue & { kind: 'enum' | 'null' }} onCommit={onCommit} enumType={enumType} nullable={nullable} />
  }
  if (value.kind === 'ref' || (value.kind === 'null' && refTargetType)) {
    return <RefDirectSelect value={value as FieldValue & { kind: 'ref' | 'null' }} onCommit={onCommit} targetType={refTargetType} nullable={nullable} />
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
  const initial = plainFieldValueText(value)
  const [text, setText] = useState(initial)
  const rowSelection = useContext(ValueRowSelectionCtx)
  useEffect(() => { setText(initial) }, [initial])

  function commit() {
    if (text === initial) return
    const next = parseFieldValueText(value, text)
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
        onBlur={() => {
          commit()
          requestAnimationFrame(() => rowSelection?.onEditingFinished?.())
        }}
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
      onBlur={() => {
        commit()
        requestAnimationFrame(() => rowSelection?.onEditingFinished?.())
      }}
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
  enumType,
  nullable = false,
}: {
  value: FieldValue & { kind: 'enum' | 'null' }
  onCommit: (next: FieldValue) => void
  /** Required when `value.kind === 'null'`: the enum type this field expects. */
  enumType?: string
  /** When true, offer a "(null)" option so the field can be cleared. */
  nullable?: boolean
}) {
  const lookups = useEditorLookups()
  const enumName = value.kind === 'enum' ? value.value.enum_name : enumType
  const [variants, setVariants] = useState<string[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const current = value.kind === 'enum' ? enumVariantText(value) : NULL_SENTINEL
  const color = enumColor(enumName ?? '')
  useEffect(() => {
    if (!enumName) { setVariants([]); return }
    let alive = true
    setLoadError(null)
    lookups.loadEnumVariants(enumName).then(r => {
      if (!alive) return
      if (r.ok) setVariants(r.value)
      else { setVariants([]); setLoadError(r.error ?? null) }
    })
    return () => { alive = false }
  }, [enumName, lookups])

  function commit(next: string) {
    if (next === NULL_SENTINEL) {
      onCommit(nullValue())
      return
    }
    if (!enumName) return
    const backingInt = value.kind === 'enum' ? value.value.value : 0n
    onCommit(enumValue(enumName, next, backingInt))
  }

  const pillClass = 'dc-pill-select dc-pill-select-enum'

  if (variants === null || variants.length === 0) {
    // No known variants — free-text fallback (skip null hint here to keep it simple)
    return (
      <span className="dc-pill-input-wrap">
        <input
          className={pillClass}
          style={{ '--enum-color': color } as React.CSSProperties}
          defaultValue={value.kind === 'enum' ? enumVariantText(value) : ''}
          aria-invalid={!!loadError}
          onBlur={e => {
            const next = e.target.value
            if (value.kind === 'enum' && next === enumVariantText(value)) return
            if (value.kind === 'null' && next === '') return
            commit(next || (nullable ? NULL_SENTINEL : ''))
          }}
        />
        {loadError && <span className="dc-load-error" title={loadError}>!</span>}
      </span>
    )
  }
  return (
    <select
      className={pillClass}
      style={{ '--enum-color': color } as React.CSSProperties}
      value={current}
      onChange={e => commit(e.target.value)}
    >
      {(nullable || value.kind === 'null') && (
        <option value={NULL_SENTINEL} disabled={!nullable}>
          {nullable ? '(null)' : '选择枚举...'}
        </option>
      )}
      {value.kind === 'enum' && !variants.includes(current) && <option value={current}>{current}</option>}
      {variants.map(v => <option key={v} value={v}>{v}</option>)}
    </select>
  )
}

const NULL_SENTINEL = '__cfd_null__'

export function RefDirectSelect({
  value,
  onCommit,
  targetType,
  autoFocus = false,
  nullable = false,
}: {
  value: FieldValue & { kind: 'ref' | 'null' }
  onCommit: (next: FieldValue) => void
  targetType?: string
  autoFocus?: boolean
  /** When true, offer a "(null)" option so the field can be cleared. */
  nullable?: boolean
}) {
  const lookups = useEditorLookups()
  const [targets, setTargets] = useState<{ key: string; label: string }[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const currentKey = value.kind === 'ref' ? value.value : ''
  const selectedValue = value.kind === 'null' ? NULL_SENTINEL : currentKey

  useEffect(() => {
    if (!targetType) {
      setTargets(null)
      setLoadError(null)
      return
    }
    let alive = true
    setTargets(null)
    setLoadError(null)
    lookups.loadRefTargets(targetType).then(r => {
      if (!alive) return
      if (r.ok) {
        setTargets(r.value.map(target => ({
          key: target.coordinate.key,
          label: `${target.coordinate.actual_type}.${target.coordinate.key}`,
        })))
      } else {
        setTargets([])
        setLoadError(r.error ?? null)
      }
    })
    return () => { alive = false }
  }, [targetType, lookups])

  function commit(key: string) {
    if (key === NULL_SENTINEL) {
      if (value.kind !== 'null') onCommit(nullValue())
      return
    }
    if (key !== currentKey) {
      onCommit(refValue(key))
    }
  }

  if (targetType && targets !== null && targets.length > 0) {
    const hasCurrent = value.kind === 'ref' && !!value.value && targets.some(target => target.key === value.value)
    return (
      <span className="dc-pill-wrap dc-pill-wrap-ref">
        <select
          className="dc-pill-select dc-pill-select-ref dc-pill-select-inwrap"
          value={selectedValue}
          autoFocus={autoFocus}
          title={targetType}
          onChange={e => commit(e.target.value)}
        >
          {(nullable || value.kind === 'null') && (
            <option value={NULL_SENTINEL} disabled={!nullable}>
              {nullable ? '(null)' : '选择引用...'}
            </option>
          )}
          {value.kind === 'ref' && !hasCurrent && value.value && <option value={value.value}>{value.value}</option>}
          {targets.map(target => (
            <option key={target.label} value={target.key} title={target.label}>
              {target.key}
            </option>
          ))}
        </select>
      </span>
    )
  }

  return (
    <span className="dc-pill-wrap dc-pill-wrap-ref">
      <span className="dc-pill-prefix" aria-hidden>&amp;</span>
      <input
        className="dc-pill-select dc-pill-select-ref dc-pill-select-inwrap"
        defaultValue={currentKey}
        autoFocus={autoFocus}
        placeholder="key"
        aria-invalid={!!loadError}
        onBlur={e => commit(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          if (e.key === 'Escape') (e.target as HTMLInputElement).blur()
        }}
      />
      {loadError && <span className="dc-load-error" title={loadError}>!</span>}
    </span>
  )
}

export function InlineEditor({
  value,
  onCommit,
  onCancel,
  targetType,
}: {
  value: FieldValue
  onCommit: (next: FieldValue) => void
  onCancel: () => void
  targetType?: string
}) {
  const initial = plainFieldValueText(value)
  const [editVal, setEditVal] = useState(initial)

  function commit(raw: string) {
    const next = parseFieldValueText(value, raw)
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
    return <RefSelect value={value} onCommit={onCommit} onCancel={onCancel} targetType={targetType} />
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
  const lookups = useEditorLookups()
  const [variants, setVariants] = useState<string[] | null>(null)
  useEffect(() => {
    let alive = true
    lookups.loadEnumVariants(value.value.enum_name).then(r => { if (alive) setVariants(r.ok ? r.value : []) })
    return () => { alive = false }
  }, [value.value.enum_name, lookups])

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
  targetType,
}: {
  value: FieldValue & { kind: 'ref' }
  onCommit: (next: FieldValue) => void
  onCancel: () => void
  targetType?: string
}) {
  const lookups = useEditorLookups()
  const [val, setVal] = useState(value.value)
  const [targets, setTargets] = useState<{ key: string; label: string }[] | null>(null)
  useEffect(() => { setVal(value.value) }, [value.value])
  useEffect(() => {
    if (!targetType) {
      setTargets(null)
      return
    }
    let alive = true
    setTargets(null)
    lookups.loadRefTargets(targetType).then(r => {
      if (!alive) return
      setTargets(r.ok ? r.value.map(target => ({
        key: target.coordinate.key,
        label: `${target.coordinate.actual_type}.${target.coordinate.key}`,
      })) : [])
    })
    return () => { alive = false }
  }, [targetType, lookups])

  if (targetType && targets === null) {
    return <input className="dc-input dc-input-ref-select" value={val} disabled placeholder="加载中..." />
  }
  const loadedTargets = targets ?? []
  if (targetType && loadedTargets.length > 0) {
    return (
      <select
        className="dc-input dc-input-ref-select"
        defaultValue={value.value}
        autoFocus
        onChange={e => onCommit(refValue(e.target.value))}
        onKeyDown={e => { if (e.key === 'Escape') onCancel() }}
      >
        {!value.value && <option value="" disabled>选择...</option>}
        {value.value && !loadedTargets.some(target => target.key === value.value) && <option value={value.value}>{value.value}</option>}
        {loadedTargets.map(target => <option key={target.label} value={target.key}>{target.label}</option>)}
      </select>
    )
  }

  return (
    <input
      className="dc-input dc-input-ref-select"
      value={val}
      autoFocus
      placeholder="key"
      onChange={e => setVal(e.target.value)}
      onBlur={() => onCommit(refValue(val))}
      onKeyDown={e => {
        if (e.key === 'Enter') onCommit(refValue(val))
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
  onCollectionEdit,
  isSpread,
  spreadInfo,
  declaredType,
  refTargetType,
  valueAnnotation,
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
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  isSpread?: boolean
  spreadInfo?: SpreadInfo
  declaredType?: string
  refTargetType?: string
  valueAnnotation?: FieldAnnotation | null
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onRowToggle?: (path: string, expanded: boolean) => void
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const autoExpandPaths = useContext(AutoExpandCtx)
  const controlledExpansion = useContext(ControlledExpansionCtx)
  const shouldAutoExpand = !!pathKey && autoExpandPaths.has(pathKey)
  const [localExpanded, setLocalExpanded] = useState(shouldAutoExpand)
  const expanded = pathKey && controlledExpansion
    ? controlledExpansion.has(pathKey)
    : localExpanded
  useEffect(() => {
    if (shouldAutoExpand && !expanded) {
      if (!controlledExpansion) setLocalExpanded(true)
      if (pathKey) onRowToggle?.(pathKey, true)
    }
    // Only fire when the auto-expand set changes for this row. If the user
    // then manually collapses it, we don't force it back open.
  }, [shouldAutoExpand])
  const summary = headerSummary(value, declaredType)
  const count = childCount(value)
  const childAnnotation = (key: string | number) => annotationChild(valueAnnotation, key)
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = spreadHint || (diag.messages.join('\n') || undefined)
  const rowSelection = useContext(ValueRowSelectionCtx)
  const selected = sameFieldPath(rowSelection?.selectedFieldPath, fieldPath)

  function toggle() {
    const next = !expanded
    if (!controlledExpansion) setLocalExpanded(next)
    if (pathKey) onRowToggle?.(pathKey, next)
  }

  return (
    <>
      <div className={`dc-row dc-row-foldout${selected ? ' keyboard-selected' : ''}${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ' dc-row-diag dc-row-diag-' + diag.sev : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} data-depth={depth} data-field-name={depth === 0 ? label : undefined} data-field-path={pathKey} data-field-path-wire={JSON.stringify(fieldPath)} title={rowTitle} onMouseDown={() => rowSelection?.onSelectValue?.(fieldPath)} onClick={toggle} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
        <div className="dc-row-label" style={{ paddingLeft: depth * INDENT_PX + 4 }}>
          {leading}
          <span className="dc-fold-arrow">
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
          </span>
          <span className="dc-row-label-text">{label}</span>
          {depth === 0 && declaredType && (
            <span className="dc-row-type" title={`类型：${declaredType}`}>{declaredType}</span>
          )}
        </div>
        <div className="dc-row-value">
          <div className="dc-row-value-inner">
            <span className="vc vc-type">{summary}</span>
            {count !== null && <span className="vc-count">{count}</span>}
          </div>
          {trailing}
        </div>
        <DiagCornerBadge severity={diag.sev} pathKey={pathKey} />
      </div>
      {expanded && (
        <>
          {value.kind === 'object' &&
            objectFields(value).map((fc) => {
              const childAnn = childAnnotation(fc.name) ?? fc.annotation
              return (
              <FieldRow
                key={fc.name}
                label={fc.name}
                value={fc.value}
                depth={depth + 1}
                onEdit={onEdit}
                onCollectionEdit={onCollectionEdit}
                fieldPath={[...fieldPath, fieldPathField(fc.name)]}
                pathKey={pathKey ? `${pathKey}.${fc.name}` : fc.name}
                onRowToggle={onRowToggle}
                declaredType={annotationDeclaredType(childAnn)}
                refTargetType={annotationRefTargetType(childAnn)}
                enumType={annotationEnumType(childAnn)}
                nullable={annotationNullable(childAnn)}
                valueAnnotation={childAnn}
              />
              )
            })}
          {value.kind === 'array' && (
            <ArrayItems
              container={value}
              depth={depth + 1}
              fieldPath={fieldPath}
              pathKey={pathKey}
              onEdit={onEdit}
              onCollectionEdit={onCollectionEdit}
              onRowToggle={onRowToggle}
              itemTemplate={annotationItem(valueAnnotation)}
              itemAnnotations={valueAnnotation?.children}
            />
          )}
          {value.kind === 'dict' &&
            value.value.map(([key, item]) => {
              const keyText = dictKeyPathText(key)
              const itemAnnotation = childAnnotation(keyText)
              const itemTemplate = annotationItem(valueAnnotation)
              const effectiveAnnotation = itemAnnotation ?? itemTemplate
              return (
              <FieldRow
                key={dictKeyText(key)}
                label={dictKeyText(key)}
                value={item}
                depth={depth + 1}
                onEdit={onEdit}
                fieldPath={[...fieldPath, fieldPathDictKey(dictKeyPathText(key))]}
                pathKey={pathKey ? `${pathKey}[${dictKeyText(key)}]` : `[${dictKeyText(key)}]`}
                onRowToggle={onRowToggle}
                declaredType={annotationDeclaredType(effectiveAnnotation)}
                refTargetType={annotationRefTargetType(effectiveAnnotation)}
                enumType={annotationEnumType(effectiveAnnotation)}
                nullable={annotationNullable(effectiveAnnotation)}
                valueAnnotation={effectiveAnnotation}
                trailing={onEdit ? (
                  <DeleteButton
                    title="删除"
                    onClick={() => onCollectionEdit?.(fieldPath, { kind: 'dict_remove', key })}
                  />
                ) : undefined}
              />
            )})}
          {onCollectionEdit && (value.kind === 'array' || value.kind === 'dict') && (
            <CollectionAddRow
              container={value}
              depth={depth + 1}
              onCollectionEdit={edit => onCollectionEdit(fieldPath, edit)}
              itemAnnotation={annotationItem(valueAnnotation)}
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

function headerSummary(v: FieldValue, declaredType?: string): string {
  switch (v.kind) {
    case 'object': return v.value.actual_type
    case 'array': return declaredType ?? (v.value[0] ? `[${typeLabelForValue(v.value[0])}]` : 'array')
    case 'dict': return v.value[0]
      ? declaredType ?? `${dictKindLabel(v.value[0][0])} -> ${typeLabelForValue(v.value[0][1])}`
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

/** If `declaredType` describes an array/dict, return an empty collection
 *  value the UI can render as if the null field were already materialized.
 *  Object types are not covered — they would need per-field defaults. */
function ArrayItems({
  container,
  depth,
  fieldPath,
  pathKey,
  onEdit,
  onCollectionEdit,
  onRowToggle,
  itemTemplate,
  itemAnnotations,
}: {
  container: FieldValue & { kind: 'array' }
  depth: number
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  onRowToggle?: (path: string, expanded: boolean) => void
  /** Element-schema template supplied by the annotator. Prefer this over the
   *  per-index children when the child hasn't accumulated its own metadata. */
  itemTemplate?: FieldAnnotation
  itemAnnotations?: { [key: string]: FieldAnnotation | undefined }
}) {
  const [dragIdx, setDragIdx] = useState<number | null>(null)
  const [overIdx, setOverIdx] = useState<number | null>(null)
  const dragArmedRef = useRef<number | null>(null)

  function dropAt(target: number) {
    if (dragIdx === null || dragIdx === target) return
    onCollectionEdit?.(fieldPath, { kind: 'array_move', from: dragIdx, to: target })
    setDragIdx(null)
    setOverIdx(null)
  }

  return (
    <>
      {container.value.map((item, i) => {
        const itemAnnotation = itemAnnotations?.[String(i)] ?? itemTemplate
        const canCollectionEdit = !!onCollectionEdit
        const dragHandle = canCollectionEdit ? <DragHandle rowIndex={i} dragArmedRef={dragArmedRef} /> : undefined
        const trailing = canCollectionEdit ? (
          <DeleteButton
            title="删除"
            onClick={() => onCollectionEdit?.(fieldPath, { kind: 'array_remove', index: i })}
          />
        ) : undefined
        return (
          <FieldRow
            key={i}
            label={`[${i}]`}
            value={item}
            depth={depth}
            onEdit={onEdit}
            onCollectionEdit={onCollectionEdit}
            fieldPath={[...fieldPath, fieldPathIndex(i)]}
            pathKey={pathKey ? `${pathKey}[${i}]` : `[${i}]`}
            onRowToggle={onRowToggle}
            declaredType={annotationDeclaredType(itemAnnotation)}
            refTargetType={annotationRefTargetType(itemAnnotation)}
            enumType={annotationEnumType(itemAnnotation)}
            nullable={annotationNullable(itemAnnotation)}
            valueAnnotation={itemAnnotation}
            leading={dragHandle}
            trailing={trailing}
            dragProps={canCollectionEdit ? {
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

function CollectionAddRow({ container, depth, onCollectionEdit, itemAnnotation }: {
  container: FieldValue & { kind: 'array' | 'dict' }
  depth: number
  onCollectionEdit: (edit: CollectionEdit) => void
  itemAnnotation?: FieldAnnotation
}) {
  const [adding, setAdding] = useState(false)
  const [dupError, setDupError] = useState<string | null>(null)
  const [addError, setAddError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const { openObjectDraft } = useObjectDraft()

  function reset() { setAdding(false); setDupError(null); setAddError(null) }

  const objectDraft = container.value.length === 0
    ? objectDraftForAnnotation(itemAnnotation)
    : null

  function addArrayItem() {
    if (objectDraft) {
      openObjectDraft({
        title: `新建 ${objectDraft.actualType}`,
        actualType: objectDraft.actualType,
        polymorphicTypes: objectDraft.polymorphicTypes,
        confirmLabel: '添加',
        onConfirm: value => onCollectionEdit({ kind: 'array_append', value }),
      })
      return
    }
    onCollectionEdit({ kind: 'array_append' })
  }

  if (container.kind === 'array') {
    return (
      <div className="dc-row dc-row-add" style={{ paddingLeft: depth * INDENT_PX + 8 }}>
        <button
          className="btn-add-item"
          disabled={busy}
          onClick={async () => {
            setAddError(null)
            setBusy(true)
            try {
              addArrayItem()
            } finally {
              setBusy(false)
            }
          }}
        >
          <Icon name="plus" size={11} /> {busy ? '添加中…' : '添加元素'}
        </button>
        {addError && <span className="dc-inline-error" role="alert">{addError}</span>}
      </div>
    )
  }

  const sampleKey: DictKey = container.value[0]?.[0] ?? { kind: 'string', value: '' }
  async function tryAdd(key: DictKey) {
    if (container.kind !== 'dict') return
    const dup = container.value.some(([entryKey]) => dictKeyEq(entryKey, key))
    if (dup) {
      setDupError(`键 "${dictKeyText(key)}" 已存在`)
      return
    }
    if (objectDraft) {
      openObjectDraft({
        title: `新建 ${objectDraft.actualType}`,
        actualType: objectDraft.actualType,
        polymorphicTypes: objectDraft.polymorphicTypes,
        confirmLabel: '添加',
        onConfirm: value => onCollectionEdit({ kind: 'dict_insert', key, value }),
      })
    } else {
      onCollectionEdit({ kind: 'dict_insert', key })
    }
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

function objectDraftForAnnotation(annotation?: FieldAnnotation): {
  actualType: string
  polymorphicTypes: string[]
} | null {
  if (!annotation || annotationRefTargetType(annotation) || annotationEnumType(annotation)) return null
  const polymorphicTypes = annotationPolymorphicTypes(annotation)
  const declaredType = annotationDeclaredType(annotation)
  const actualType = polymorphicTypes[0] ?? declaredType?.replace(/\?$/, '')
  if (!actualType || scalarDefaultForDeclaredType(actualType) !== null) return null
  return { actualType, polymorphicTypes }
}

function DictKeyEntry({ sampleKey, onCommit, onCancel }: {
  sampleKey: DictKey
  onCommit: (k: DictKey) => void
  onCancel: () => void
}) {
  const lookups = useEditorLookups()
  const [text, setText] = useState('')
  const [variants, setVariants] = useState<string[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  useEffect(() => {
    if (sampleKey.kind !== 'enum') return
    let alive = true
    setLoadError(null)
    lookups.loadEnumVariants(sampleKey.value.enum_name).then(r => {
      if (!alive) return
      if (r.ok) setVariants(r.value)
      else { setVariants([]); setLoadError(r.error ?? null) }
    })
    return () => { alive = false }
  }, [sampleKey.kind === 'enum' ? sampleKey.value.enum_name : '', lookups])

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

export function DataCardNode({
  fields,
  actualType,
  showAll,
  onToggle,
  onRowToggle,
  expandedPaths,
  onEdit,
  onCollectionEdit,
}: {
  fields: FieldCell[]
  actualType: string
  showAll: boolean
  onToggle: () => void
  onRowToggle?: (path: string, expanded: boolean) => void
  expandedPaths?: ReadonlySet<string>
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
}) {
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return (
    <div className="dc-node-card">
      <DataCardExpanded
        fields={visible}
        actualType={actualType}
        onRowToggle={onRowToggle}
        expandedPaths={expandedPaths}
        onEdit={onEdit}
        onCollectionEdit={onCollectionEdit}
      />
      {fields.length > NODE_PEEK_FIELDS && (
        <button className="dc-node-more" onClick={onToggle}>
          {showAll ? '收起' : `显示全部 (+${fields.length - NODE_PEEK_FIELDS})`}
        </button>
      )}
    </div>
  )
}
