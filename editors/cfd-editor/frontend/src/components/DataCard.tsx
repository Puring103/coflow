import {
  useState,
  useEffect,
  useRef,
  useContext,
  createContext,
  useMemo,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
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
import { typeColor, fieldTypeColor } from '../utils/typeColor'
import { useEditorLookups, useEditorNavigation } from '../utils/editContext'
import type { EditorLookupAccess } from '../utils/editContext'
import {
  collectionShapeForDeclaredType,
  parseFieldValueText,
  plainFieldValueText,
  referenceKeyText,
  scalarDefaultForDeclaredType,
  summaryOf,
} from '../value/fieldValue'
import { useObjectDraft } from './ObjectDraftHost'
import { NODE_PEEK_FIELDS } from './DataCard.geometry'
import { SearchableSelect } from './SearchableSelect'
import { PluginRendererMount, useFieldRenderer } from '../plugins'
import type { FieldRenderSurface, FieldRenderer } from '../plugins/types'
import { sameNumericValue, scrubNumericValue, type NumericFieldValue } from '../value/numericScrub'

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

const INDENT_PX = 14

function inspectorDepthStyle(depth: number): CSSProperties {
  return { '--dc-indent': `${depth * INDENT_PX}px` } as CSSProperties
}

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

/** Strip trailing `?` off a declared type string. Kept for the rare cases
 *  (null-collection detection, resolveDefaultElement scalar shorthand) that
 *  still work on the wire-formatted type string. Other schema questions
 *  should read `FieldAnnotation.item_annotation` / `.ref_target_type` /
 *  `.enum_type` instead — the backend fills those directly. */
function stripNullableType(declaredType?: string): string | undefined {
  return declaredType?.endsWith('?') ? declaredType.slice(0, -1) : declaredType
}

function dictKeyText(k: DictKey): string {
  switch (k.kind) {
    case 'string': return `"${k.value}"`
    case 'int': return String(k.value)
    case 'enum': return dictEnumVariantText(k)
  }
}

export function DataCardCompact({ value, label, declaredType, refTargetType, surface = 'table-cell', highlightQuery }: { value: FieldValue; label?: string; declaredType?: string; refTargetType?: string; surface?: FieldRenderSurface; highlightQuery?: string }) {
  const fallback = isComplexValue(value)
    ? <MarkdownValueTree value={value} label={value.kind === 'array' ? undefined : label} depth={0} highlightQuery={highlightQuery} />
    : <ValueChip value={value} refTargetType={refTargetType} highlightQuery={highlightQuery} />
  const nullable = declaredType?.endsWith('?') ?? false
  const renderer = useFieldRenderer({ value, type: declaredType ?? '', nullable, surface })
  return (
    <PluginRendererMount renderer={renderer} context={{ value, type: declaredType ?? '', nullable, surface }} fallback={fallback} />
  )
}

function MarkdownValueTree({ value, label, depth, highlightQuery }: {
  value: FieldValue & { kind: 'object' | 'array' | 'dict' }
  label?: string
  depth: number
  highlightQuery?: string
}) {
  const entries = treeEntries(value)
  const depthClass = `markdown-tree-depth-${Math.min(depth, 2)}`
  const inlineScalarArray = value.kind === 'array'
    && entries.every(entry => !isComplexValue(entry.value))

  return (
    <div className={`markdown-value-tree ${depthClass}${label ? ' has-branch-label' : ''}${inlineScalarArray ? ' inline-scalar-array' : ''}`}>
      {label && <div className="markdown-tree-branch-label">{highlightSearchText(label, highlightQuery)}</div>}
      {entries.length === 0 ? (
        <div className="markdown-tree-empty">—</div>
      ) : (
        <div className="markdown-tree-items">
          {entries.map((entry, index) => (
            <MarkdownTreeItem key={`${entry.marker}:${index}`} entry={entry} depth={depth} highlightQuery={highlightQuery} />
          ))}
        </div>
      )}
    </div>
  )
}

interface MarkdownTreeEntry {
  marker: string
  markerKind: 'plain' | 'index' | 'key'
  branchLabel?: string
  value: FieldValue
}

function MarkdownTreeItem({ entry, depth, highlightQuery }: { entry: MarkdownTreeEntry; depth: number; highlightQuery?: string }) {
  const complex = isComplexValue(entry.value) ? entry.value : null
  return (
    <div className={`markdown-tree-item${complex ? ' complex-item' : ''}`}>
      <span className={`markdown-tree-marker marker-${entry.markerKind}`}>{highlightSearchText(entry.marker, highlightQuery)}</span>
      <div className="markdown-tree-content">
        {complex ? (
          <MarkdownValueTree value={complex} label={entry.branchLabel} depth={depth + 1} highlightQuery={highlightQuery} />
        ) : (
          <span className="markdown-tree-leaf"><ValueChip value={entry.value} highlightQuery={highlightQuery} /></span>
        )}
      </div>
    </div>
  )
}

function treeEntries(value: FieldValue & { kind: 'object' | 'array' | 'dict' }): MarkdownTreeEntry[] {
  if (value.kind === 'object') {
    return Object.entries(value.value.fields)
      .filter((entry): entry is [string, FieldValue] => entry[1] !== undefined)
      .map(([fieldName, fieldValue]) => ({
        marker: '',
        markerKind: 'plain',
        branchLabel: isComplexValue(fieldValue) ? fieldName : undefined,
        value: fieldValue,
      }))
  }
  if (value.kind === 'array') {
    return value.value.map((item, index) => {
      const complex = isComplexValue(item)
      return {
        marker: complex ? `${index + 1}.` : '',
        markerKind: complex ? 'index' : 'plain',
        value: item,
      }
    })
  }
  return value.value.map(([key, item]) => ({
    marker: `${dictTreeKey(key)} →`,
    markerKind: 'key',
    value: item,
  }))
}

function isComplexValue(value: FieldValue): value is FieldValue & { kind: 'object' | 'array' | 'dict' } {
  return value.kind === 'object' || value.kind === 'array' || value.kind === 'dict'
}

function dictTreeKey(key: DictKey): string {
  switch (key.kind) {
    case 'string': return key.value
    case 'int': return String(key.value)
    case 'enum': return dictEnumVariantText(key)
  }
}

function ValueChip({ value, refTargetType, highlightQuery }: { value: FieldValue; refTargetType?: string; highlightQuery?: string }) {
  const navigation = useEditorNavigation()
  switch (value.kind) {
    case 'null':
      return <span className="vc vc-null">{highlightSearchText('null', highlightQuery)}</span>
    case 'bool':
      return (
        <span className={`vc vc-bool${value.value ? ' on' : ''}`}>
          <input type="checkbox" className="dc-checkbox dc-checkbox-ro" checked={value.value} readOnly tabIndex={-1} />
        </span>
      )
    case 'int':
    case 'float':
      return <span className="vc vc-num">{highlightSearchText(String(value.value), highlightQuery)}</span>
    case 'string':
      return <span className="vc vc-str">{highlightSearchText(summaryOf(value), highlightQuery)}</span>
    case 'enum':
      return (
        <span className="vc vc-enum">
          <span className="vc-enum-dot" />
          {highlightSearchText(enumVariantText(value), highlightQuery)}
        </span>
      )
    case 'ref':
      const refKey = referenceKeyText(value.value)
      const refColor = typeColor(refTargetType ?? 'ref')
      return (
        <span
          className={`vc vc-ref${refTargetType && navigation ? ' vc-ref-navigable' : ''}`}
          style={{ '--ref-color': refColor } as CSSProperties}
          title={refTargetType ? `${refKey} · Ctrl/Cmd+点击跳转` : refKey}
          onClick={event => {
            if (!refTargetType || !navigation || (!event.ctrlKey && !event.metaKey)) return
            event.preventDefault()
            event.stopPropagation()
            navigation.openReference(refTargetType, refKey)
          }}
        >
          <Icon name="dot" size={9} />
          <span className="vc-ref-key">{highlightSearchText(refKey, highlightQuery)}</span>
        </span>
      )
    case 'object':
      return <span className="vc vc-obj">{highlightSearchText(value.value.actual_type, highlightQuery)}</span>
    case 'array':
      return <span className="vc vc-arr">{highlightSearchText(summaryOf(value), highlightQuery)}</span>
    case 'dict':
      return <span className="vc vc-dict">{highlightSearchText(summaryOf(value), highlightQuery)}</span>
  }
}

export function highlightSearchText(text: string, query?: string): ReactNode {
  const needle = query?.trim()
  if (!needle) return text
  const lowerText = text.toLocaleLowerCase()
  const lowerNeedle = needle.toLocaleLowerCase()
  const parts: ReactNode[] = []
  let offset = 0
  let index = lowerText.indexOf(lowerNeedle, offset)
  while (index !== -1) {
    if (index > offset) parts.push(text.slice(offset, index))
    parts.push(<mark className="search-highlight" key={index}>{text.slice(index, index + needle.length)}</mark>)
    offset = index + needle.length
    index = lowerText.indexOf(lowerNeedle, offset)
  }
  if (offset < text.length) parts.push(text.slice(offset))
  return parts.length > 0 ? parts : text
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
  selectedActionPathWire?: string | null
  onSelectValue?: (fieldPath: FieldPathSegment[]) => void
  onSelectAction?: (pathWire: string) => void
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
      const current = prefixes.get(p)
      prefixes.set(p, severity === 'error' || current === 'error' ? 'error' : 'warning')
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
  selectedActionPathWire?: string | null
  onSelectValue?: (fieldPath: FieldPathSegment[]) => void
  onSelectAction?: (pathWire: string) => void
  onEditingFinished?: () => void
  flattenSingleComplexField?: boolean
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
  selectedActionPathWire,
  onSelectValue,
  onSelectAction,
  onEditingFinished,
  flattenSingleComplexField = false,
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
        const nullCollectionShape = fc.value.kind === 'null'
          ? collectionShapeForDeclaredType(declaredType)
          : null
        const displayedValue = nullCollectionShape ?? fc.value
        if (
          flattenSingleComplexField
          && fields.length === 1
          && displayedValue.kind === 'object'
        ) {
          return (
            <ComplexValueChildren
              key={fc.name}
              value={displayedValue}
              depth={depth}
              fieldPath={[fieldPathField(fc.name)]}
              pathKey={pathPrefix ? `${pathPrefix}.${fc.name}` : fc.name}
              onEdit={fieldEdit}
              onCollectionEdit={fieldEdit ? onCollectionEdit : undefined}
              onRowToggle={onRowToggle}
              valueAnnotation={fc.annotation}
            />
          )
        }
        return (
          <FieldRow
            key={fc.name}
            label={fc.annotation?.label ?? fc.name}
            fieldName={fc.name}
            description={fc.annotation?.description ?? undefined}
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
    <ValueRowSelectionCtx.Provider value={{ selectedFieldPath, selectedActionPathWire, onSelectValue, onSelectAction, onEditingFinished }}>
      <ControlledExpansionCtx.Provider value={expandedPaths ?? null}>
        <AutoExpandCtx.Provider value={autoExpandSet}>{body}</AutoExpandCtx.Provider>
      </ControlledExpansionCtx.Provider>
    </ValueRowSelectionCtx.Provider>
  )
  return ctx ? <DiagCtx.Provider value={ctx}>{wrapped}</DiagCtx.Provider> : wrapped
}

function rowDiagSeverity(pathKey: string | undefined): {
  sev: 'error' | 'warning' | 'info' | null
  messages: string[]
  exact: boolean
} {
  const ctx = useContext(DiagCtx)
  if (!ctx || !pathKey) return { sev: null, messages: [], exact: false }
  const exact = ctx.byPath.get(pathKey)
  const prefix = ctx.prefixes.get(pathKey)
  if (!exact && !prefix) return { sev: null, messages: [], exact: false }
  const sevs: ('error' | 'warning' | 'info')[] = []
  if (exact) sevs.push(strongest(exact))
  if (prefix) sevs.push(prefix)
  let sev: 'error' | 'warning' | 'info' = 'info'
  for (const s of sevs) if (severityRank(s) > severityRank(sev)) sev = s
  return {
    sev,
    messages: exact ? exact.map(d => d.message) : [],
    exact: !!exact?.some(d => normalizedDiagnosticSeverity(d.severity) !== 'info'),
  }
}

function FieldRow({
  label,
  fieldName,
  description,
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
  collectionItem,
}: {
  label: string
  fieldName?: string
  description?: string
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
  collectionItem?: boolean
}) {
  const pluginRenderer = useFieldRenderer({
    value,
    type: declaredType ?? '',
    nullable: !!nullable,
    surface: 'record-foldout-header',
  })
  const isComplex = value.kind === 'object' || value.kind === 'array' || value.kind === 'dict'
  // A `null` value on a field whose declared type is an array/dict/object
  // should still be treated as expandable, so the user can just click
  // "add element" instead of first coercing null → empty collection by
  // hand. The materialization happens lazily when the user hits add.
  const nullCollectionShape = value.kind === 'null' ? collectionShapeForDeclaredType(declaredType) : null
  const displayValue = nullCollectionShape ?? value
  const canExpand = isComplex || nullCollectionShape !== null
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
        fieldName={fieldName}
        description={description}
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
        collectionItem={collectionItem}
        pluginRenderer={pluginRenderer}
        pluginContext={pluginRenderer ? { value, type: declaredType ?? '', nullable: !!nullable, surface: 'record-foldout-header' } : undefined}
      />
    )
  }
  return (
    <ScalarFieldRow
      label={label}
      fieldName={fieldName}
      description={description}
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
      collectionItem={collectionItem}
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
        return enumValue(enumType, variants.value[0].name, 0n)
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
  fieldName,
  description,
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
  collectionItem,
  pluginRenderer,
  pluginContext,
}: {
  label: string
  fieldName?: string
  description?: string
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
  collectionItem?: boolean
  pluginRenderer?: FieldRenderer
  pluginContext?: Parameters<typeof useFieldRenderer>[0]
}) {
  const isScalar = value.kind === 'bool' || value.kind === 'int' || value.kind === 'float'
    || value.kind === 'string' || value.kind === 'enum' || value.kind === 'ref'
  const resolvedRefTarget = refTargetType
  const isNullDropdown = value.kind === 'null' && !!(enumType || resolvedRefTarget)
  const canEdit = !pluginRenderer && (isScalar || isNullDropdown) && !!onCommit
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = [description, spreadHint, declaredType ? `类型：${declaredType}` : null, ...diag.messages]
    .filter(Boolean).join('\n') || undefined
  const rowSelection = useContext(ValueRowSelectionCtx)
  const selected = sameFieldPath(rowSelection?.selectedFieldPath, fieldPath)
  const numericValue = value.kind === 'int' || value.kind === 'float' ? value : null
  const numericScrubEnabled = !!numericValue && canEdit && !dragProps
  const [scrubPreview, setScrubPreview] = useState<NumericFieldValue | null>(null)
  const scrubCleanupRef = useRef<(() => void) | null>(null)
  useEffect(() => () => scrubCleanupRef.current?.(), [])

  function beginNumericScrub(event: ReactPointerEvent<HTMLDivElement>) {
    if (!numericValue || !numericScrubEnabled || !onCommit || event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    rowSelection?.onSelectValue?.(fieldPath)
    scrubCleanupRef.current?.()
    const pointerId = event.pointerId
    const startX = event.clientX
    const start = numericValue
    let latest = start
    let finished = false

    const cleanup = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onCancel)
      document.body.classList.remove('dc-numeric-scrubbing')
      if (scrubCleanupRef.current === cleanup) scrubCleanupRef.current = null
    }
    const finish = (commit: boolean) => {
      if (finished) return
      finished = true
      cleanup()
      setScrubPreview(null)
      if (commit && !sameNumericValue(start, latest)) onCommit(latest)
    }
    const onMove = (pointerEvent: PointerEvent) => {
      if (pointerEvent.pointerId !== pointerId) return
      pointerEvent.preventDefault()
      latest = scrubNumericValue(start, pointerEvent.clientX - startX, {
        shiftKey: pointerEvent.shiftKey,
        altKey: pointerEvent.altKey,
      })
      setScrubPreview(latest)
    }
    const onUp = (pointerEvent: PointerEvent) => {
      if (pointerEvent.pointerId === pointerId) finish(true)
    }
    const onCancel = (pointerEvent: PointerEvent) => {
      if (pointerEvent.pointerId === pointerId) finish(false)
    }

    scrubCleanupRef.current = cleanup
    document.body.classList.add('dc-numeric-scrubbing')
    setScrubPreview(start)
    window.addEventListener('pointermove', onMove, { passive: false })
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onCancel)
  }

  const displayedValue = scrubPreview ?? value

  return (
    <div className={`dc-row dc-row-field${collectionItem ? ' dc-row-item' : ''}${selected ? ' keyboard-selected' : ''}${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ` dc-row-diag dc-row-diag-${diag.sev}${diag.exact ? ' dc-row-diag-exact' : ' dc-row-diag-summary'}` : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} style={inspectorDepthStyle(depth)} data-depth={depth} data-field-name={depth === 0 ? fieldName : undefined} data-field-path={pathKey} data-field-path-wire={JSON.stringify(fieldPath)} data-value-kind={value.kind} data-bool-value={value.kind === 'bool' ? String(value.value) : undefined} data-keyboard-editable={canEdit || undefined} title={rowTitle} onMouseDown={() => rowSelection?.onSelectValue?.(fieldPath)} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
      <div
        className={`dc-row-label${numericScrubEnabled ? ' dc-numeric-scrub-label' : ''}`}
        onPointerDown={numericScrubEnabled ? beginNumericScrub : undefined}
      >
        {leading}
        <span className="dc-row-label-text">{label}</span>
      </div>
      <div className="dc-row-value">
        <div className="dc-row-value-inner">
          {pluginRenderer && pluginContext ? (
            <PluginRendererMount renderer={pluginRenderer} context={pluginContext} fallback={<ValueChip value={displayedValue} refTargetType={resolvedRefTarget} />} />
          ) : canEdit ? (
            <DirectEditor value={displayedValue} onCommit={onCommit!} declaredType={declaredType} refTargetType={resolvedRefTarget} enumType={enumType} nullable={nullable} />
          ) : (
            <DataCardCompact value={displayedValue} label={label} declaredType={declaredType} refTargetType={resolvedRefTarget} />
          )}
        </div>
      </div>
      <div className="dc-row-actions">
        {trailing}
        <DiagCornerBadge severity={diag.sev} pathKey={pathKey} />
      </div>
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
  declaredType,
  refTargetType,
  enumType,
  nullable,
}: {
  value: FieldValue
  onCommit: (next: FieldValue) => void
  declaredType?: string
  refTargetType?: string
  enumType?: string
  nullable?: boolean
}) {
  const rowSelection = useContext(ValueRowSelectionCtx)
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
    return <EnumDirectSelect value={value as FieldValue & { kind: 'enum' | 'null' }} onCommit={onCommit} onExit={rowSelection?.onEditingFinished} enumType={enumType} nullable={nullable} />
  }
  if (value.kind === 'ref' || (value.kind === 'null' && refTargetType)) {
    return <RefDirectSelect value={value as FieldValue & { kind: 'ref' | 'null' }} onCommit={onCommit} onExit={rowSelection?.onEditingFinished} targetType={refTargetType} nullable={nullable} />
  }
  if (value.kind === 'int' || value.kind === 'float' || value.kind === 'string') {
    return <TextDirectInput value={value} onCommit={onCommit} color={fieldTypeColor(declaredType ?? value.kind)} />
  }
  return <ValueChip value={value} />
}

function TextDirectInput({
  value,
  onCommit,
  color,
}: {
  value: FieldValue & { kind: 'int' | 'float' | 'string' }
  onCommit: (next: FieldValue) => void
  color: string
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
      className="dc-input dc-input-flat dc-input-themed"
      style={{ '--field-color': color } as CSSProperties}
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
  onExit,
  enumType,
  autoFocus = false,
  nullable = false,
  variant = 'pill',
}: {
  value: FieldValue & { kind: 'enum' | 'null' }
  onCommit: (next: FieldValue) => void
  onExit?: () => void
  /** Required when `value.kind === 'null'`: the enum type this field expects. */
  enumType?: string
  autoFocus?: boolean
  /** When true, offer a "(null)" option so the field can be cleared. */
  nullable?: boolean
  variant?: 'pill' | 'input'
}) {
  const lookups = useEditorLookups()
  const enumName = value.kind === 'enum' ? value.value.enum_name : enumType
  const [variants, setVariants] = useState<{ name: string, label: string | null, description: string | null }[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const current = value.kind === 'enum' ? enumVariantText(value) : NULL_SENTINEL
  const color = fieldTypeColor(enumName ?? 'enum')
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

  const inputClass = variant === 'input'
    ? 'dc-input'
    : 'dc-pill-select dc-pill-select-enum'

  if (variants === null && variant === 'input') {
    return <input className={inputClass} value={current} disabled placeholder="加载中..." />
  }
  if (variants === null || variants.length === 0) {
    const input = (
      <input
        className={inputClass}
        style={{ '--enum-color': color } as React.CSSProperties}
        defaultValue={value.kind === 'enum' ? enumVariantText(value) : ''}
        autoFocus={autoFocus}
        aria-invalid={!!loadError}
        onBlur={event => {
          const next = event.target.value
          if (!(value.kind === 'enum' && next === enumVariantText(value))
            && !(value.kind === 'null' && next === '')) {
            commit(next || (nullable ? NULL_SENTINEL : ''))
          }
          requestAnimationFrame(() => onExit?.())
        }}
        onKeyDown={event => {
          if (event.key === 'Escape') {
            event.currentTarget.value = value.kind === 'enum' ? enumVariantText(value) : ''
            event.currentTarget.blur()
          }
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
      />
    )
    return variant === 'input'
      ? input
      : <span className="dc-pill-input-wrap">
        {input}
        {loadError && <span className="dc-load-error" title={loadError}>!</span>}
      </span>
  }
  return (
    <SearchableSelect
      className={inputClass}
      style={{ '--enum-color': color } as React.CSSProperties}
      value={value.kind === 'null' && !nullable ? '' : current}
      autoFocus={autoFocus}
      placeholder="选择枚举..."
      options={[
        ...(nullable ? [{ value: NULL_SENTINEL }] : []),
        ...(value.kind === 'enum' && !variants.some(v => v.name === current) ? [{ value: current }] : []),
        ...variants.map(v => ({ value: v.name, label: v.label ?? v.name, description: v.description ?? undefined })),
      ]}
      onCommit={commit}
      onExit={onExit}
    />
  )
}

const NULL_SENTINEL = '(null)'

export function RefDirectSelect({
  value,
  onCommit,
  onExit,
  targetType,
  autoFocus = false,
  nullable = false,
  variant = 'pill',
}: {
  value: FieldValue & { kind: 'ref' | 'null' }
  onCommit: (next: FieldValue) => void
  onExit?: () => void
  targetType?: string
  autoFocus?: boolean
  /** When true, offer a "(null)" option so the field can be cleared. */
  nullable?: boolean
  variant?: 'pill' | 'input'
}) {
  const lookups = useEditorLookups()
  const navigation = useEditorNavigation()
  const [targets, setTargets] = useState<{ key: string; label: string }[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const currentKey = value.kind === 'ref' ? referenceKeyText(value.value) : ''
  const selectedValue = value.kind === 'null' ? NULL_SENTINEL : currentKey
  const color = typeColor(targetType ?? 'ref')
  const inputClass = variant === 'input'
    ? 'dc-input dc-input-ref-select'
    : 'dc-pill-select dc-pill-select-ref dc-pill-select-inwrap'

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
          label: target.coordinate.key,
        })))
      } else {
        setTargets([])
        setLoadError(r.error ?? null)
      }
    })
    return () => { alive = false }
  }, [targetType, lookups])

  if (targetType && targets === null && variant === 'input') {
    return <input className={inputClass} style={{ '--ref-color': color } as CSSProperties} value={currentKey} disabled placeholder="加载中..." />
  }

  function commit(key: string) {
    if (key === NULL_SENTINEL) {
      if (value.kind !== 'null') onCommit(nullValue())
      return
    }
    if (value.kind !== 'ref' || key !== referenceKeyText(value.value)) {
      onCommit(refValue(key))
    }
  }

  if (targetType && targets !== null && targets.length > 0) {
    const hasCurrent = value.kind === 'ref' && !!currentKey && targets.some(target => target.key === currentKey)
    const select = (
      <SearchableSelect
        className={inputClass}
        style={{ '--ref-color': color } as CSSProperties}
        value={value.kind === 'null' && !nullable ? '' : selectedValue}
        autoFocus={autoFocus}
        title={targetType}
        placeholder="选择引用..."
        options={[
          ...(nullable ? [{ value: NULL_SENTINEL }] : []),
          ...(value.kind === 'ref' && !hasCurrent && currentKey ? [{ value: currentKey }] : []),
          ...targets.map(target => ({ value: target.key, label: target.label })),
        ]}
        onCommit={commit}
        onExit={onExit}
        onModifiedClick={value.kind === 'ref' && targetType && navigation
          ? () => navigation.openReference(targetType, currentKey)
          : undefined}
      />
    )
    return variant === 'input'
      ? select
      : <span className="dc-pill-wrap dc-pill-wrap-ref" style={{ '--ref-color': color } as CSSProperties}>{select}</span>
  }

  const input = (
    <input
      className={inputClass}
      style={{ '--ref-color': color } as CSSProperties}
      defaultValue={currentKey}
      autoFocus={autoFocus}
      placeholder="key"
      aria-invalid={!!loadError}
      onBlur={event => {
        commit(event.target.value)
        requestAnimationFrame(() => onExit?.())
      }}
        onClick={event => {
          if (
            value.kind === 'ref'
            && targetType
            && navigation
            && (event.ctrlKey || event.metaKey)
          ) {
            event.preventDefault()
            event.stopPropagation()
            navigation.openReference(targetType, currentKey)
          }
        }}
      onKeyDown={event => {
        if (event.key === 'Escape') event.currentTarget.value = currentKey
        if (event.key === 'Enter' || event.key === 'Escape') event.currentTarget.blur()
      }}
    />
  )
  return variant === 'input'
    ? input
    : <span className="dc-pill-wrap dc-pill-wrap-ref" style={{ '--ref-color': color } as CSSProperties}>
      {input}
      {loadError && <span className="dc-load-error" title={loadError}>!</span>}
    </span>
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
      <EnumDirectSelect
        value={value}
        onCommit={onCommit}
        onExit={onCancel}
        autoFocus
        variant="input"
      />
    )
  }
  if (value.kind === 'ref') {
    return <RefDirectSelect value={value} onCommit={onCommit} onExit={onCancel} targetType={targetType} autoFocus variant="input" />
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

function ExpandableRow({
  label,
  fieldName,
  description,
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
  collectionItem,
}: {
  label: string
  fieldName?: string
  description?: string
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
  collectionItem?: boolean
  pluginRenderer?: FieldRenderer
  pluginContext?: Parameters<typeof useFieldRenderer>[0]
}) {
  const autoExpandPaths = useContext(AutoExpandCtx)
  const controlledExpansion = useContext(ControlledExpansionCtx)
  const shouldAutoExpand = !!pathKey && autoExpandPaths.has(pathKey)
  const [localExpanded, setLocalExpanded] = useState(shouldAutoExpand)
  const staticObjectItem = !!collectionItem && value.kind === 'object'
  const expanded = staticObjectItem
    ? true
    : pathKey && controlledExpansion
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
  const count = childCount(value)
  const diag = rowDiagSeverity(pathKey)
  const spreadHint = spreadHintText(spreadInfo)
  const rowTitle = [description, spreadHint, declaredType ? `类型：${declaredType}` : null, ...diag.messages]
    .filter(Boolean).join('\n') || undefined
  const rowSelection = useContext(ValueRowSelectionCtx)
  const selected = sameFieldPath(rowSelection?.selectedFieldPath, fieldPath)
  const structureClass = collectionItem
    ? ' dc-row-item'
    : value.kind === 'array' || value.kind === 'dict'
      ? ' dc-row-collection'
      : ' dc-row-object'
  const inlineSingletonItem = value.kind === 'array'
    && value.value.length === 1
    && value.value[0]?.kind === 'object'

  function toggle() {
    if (staticObjectItem) return
    const next = !expanded
    if (!controlledExpansion) setLocalExpanded(next)
    if (pathKey) onRowToggle?.(pathKey, next)
  }

  function editCollection(edit: CollectionEdit) {
    if (!onCollectionEdit) return
    if (!expanded) {
      if (!controlledExpansion) setLocalExpanded(true)
      if (pathKey) onRowToggle?.(pathKey, true)
    }
    onCollectionEdit(fieldPath, edit)
  }

  return (
    <div className={`dc-group${collectionItem ? ' dc-group-item' : ''}${value.kind === 'array' || value.kind === 'dict' ? ' dc-group-collection' : ' dc-group-object'}`}>
      <div className={`dc-row dc-row-structure${staticObjectItem ? ' dc-row-static-group' : ' dc-row-foldout'}${structureClass}${selected ? ' keyboard-selected' : ''}${isSpread ? ' dc-row-spread' : ''}${diag.sev ? ` dc-row-diag dc-row-diag-${diag.sev}${diag.exact ? ' dc-row-diag-exact' : ' dc-row-diag-summary'}` : ''}${dragProps?.extraClass ? ' ' + dragProps.extraClass : ''}`} style={inspectorDepthStyle(depth)} data-depth={depth} data-field-name={depth === 0 ? fieldName : undefined} data-field-path={pathKey} data-field-path-wire={JSON.stringify(fieldPath)} data-value-kind={value.kind} data-keyboard-editable={!!onEdit || undefined} title={rowTitle} onMouseDown={() => rowSelection?.onSelectValue?.(fieldPath)} onClick={staticObjectItem ? undefined : toggle} {...(dragProps && { onDragStart: dragProps.onDragStart, onDragOver: dragProps.onDragOver, onDragLeave: dragProps.onDragLeave, onDrop: dragProps.onDrop, onDragEnd: dragProps.onDragEnd, draggable: dragProps.draggable })}>
        <div className="dc-row-label">
          {leading}
          {!staticObjectItem && (
            <span className="dc-fold-arrow">
              <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
            </span>
          )}
          <span className="dc-row-label-text">{label}</span>
        </div>
        <div className="dc-row-value">
          <div className="dc-row-value-inner">
            {count !== null && <span className="vc-count">{count}</span>}
          </div>
        </div>
        <div className="dc-row-actions">
          {onCollectionEdit && (value.kind === 'array' || value.kind === 'dict') && (
            <CollectionAddControl
              container={value}
              depth={depth}
              fieldPath={fieldPath}
              onCollectionEdit={editCollection}
              itemAnnotation={annotationItem(valueAnnotation)}
            />
          )}
          {onCollectionEdit && value.kind === 'array' && inlineSingletonItem && (
            <DeleteButton
              title="删除唯一元素"
              onClick={() => onCollectionEdit(fieldPath, { kind: 'array_remove', index: 0 })}
            />
          )}
          {trailing}
          <DiagCornerBadge severity={diag.sev} pathKey={pathKey} />
        </div>
      </div>
      {expanded && (
        <div className="dc-group-body">
          <ComplexValueChildren
            value={value}
            depth={depth + 1}
            fieldPath={fieldPath}
            pathKey={pathKey}
            onEdit={onEdit}
            onCollectionEdit={onCollectionEdit}
            onRowToggle={onRowToggle}
            valueAnnotation={valueAnnotation}
          />
        </div>
      )}
    </div>
  )
}

function ComplexValueChildren({
  value,
  depth,
  fieldPath,
  pathKey,
  onEdit,
  onCollectionEdit,
  onRowToggle,
  valueAnnotation,
  firstChildTrailing,
}: {
  value: FieldValue
  depth: number
  fieldPath: FieldPathSegment[]
  pathKey?: string
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  onRowToggle?: (path: string, expanded: boolean) => void
  valueAnnotation?: FieldAnnotation | null
  firstChildTrailing?: ReactNode
}) {
  if (value.kind !== 'object' && value.kind !== 'array' && value.kind !== 'dict') return null
  const childAnnotation = (key: string | number) => annotationChild(valueAnnotation, key)
  return (
    <>
      {value.kind === 'object' && objectFields(value).map((fc, index) => {
        const annotation = childAnnotation(fc.name) ?? fc.annotation
        return (
          <FieldRow
            key={fc.name}
            label={fc.name}
            value={fc.value}
            depth={depth}
            onEdit={onEdit}
            onCollectionEdit={onCollectionEdit}
            fieldPath={[...fieldPath, fieldPathField(fc.name)]}
            pathKey={pathKey ? `${pathKey}.${fc.name}` : fc.name}
            onRowToggle={onRowToggle}
            declaredType={annotationDeclaredType(annotation)}
            refTargetType={annotationRefTargetType(annotation)}
            enumType={annotationEnumType(annotation)}
            nullable={annotationNullable(annotation)}
            valueAnnotation={annotation}
            trailing={index === 0 ? firstChildTrailing : undefined}
          />
        )
      })}
      {value.kind === 'array' && (
        <ArrayItems
          container={value}
          depth={depth}
          fieldPath={fieldPath}
          pathKey={pathKey}
          onEdit={onEdit}
          onCollectionEdit={onCollectionEdit}
          onRowToggle={onRowToggle}
          itemTemplate={annotationItem(valueAnnotation)}
          itemAnnotations={valueAnnotation?.children}
        />
      )}
      {value.kind === 'dict' && value.value.map(([key, item]) => {
        const annotation = childAnnotation(dictKeyPathText(key)) ?? annotationItem(valueAnnotation)
        return (
          <FieldRow
            key={dictKeyText(key)}
            label={dictKeyText(key)}
            value={item}
            depth={depth}
            onEdit={onEdit}
            fieldPath={[...fieldPath, fieldPathDictKey(dictKeyPathText(key))]}
            pathKey={pathKey ? `${pathKey}[${dictKeyText(key)}]` : `[${dictKeyText(key)}]`}
            onRowToggle={onRowToggle}
            declaredType={annotationDeclaredType(annotation)}
            refTargetType={annotationRefTargetType(annotation)}
            enumType={annotationEnumType(annotation)}
            nullable={annotationNullable(annotation)}
            valueAnnotation={annotation}
            trailing={onEdit ? (
              <DeleteButton
                title="删除"
                onClick={() => onCollectionEdit?.(fieldPath, { kind: 'dict_remove', key })}
              />
            ) : undefined}
          />
        )
      })}
    </>
  )
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

  const onlyItem = container.value[0]
  if (container.value.length === 1 && onlyItem?.kind === 'object') {
    const itemAnnotation = itemAnnotations?.['0'] ?? itemTemplate
    return (
      <ComplexValueChildren
        value={onlyItem}
        depth={depth}
        fieldPath={[...fieldPath, fieldPathIndex(0)]}
        pathKey={pathKey ? `${pathKey}[0]` : '[0]'}
        onEdit={onEdit}
        onCollectionEdit={onCollectionEdit}
        onRowToggle={onRowToggle}
        valueAnnotation={itemAnnotation}
      />
    )
  }

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
        const itemPath = [...fieldPath, fieldPathIndex(i)]
        const itemPathKey = pathKey ? `${pathKey}[${i}]` : `[${i}]`
        const trailing = canCollectionEdit ? (
          <DeleteButton
            title="删除"
            onClick={() => onCollectionEdit?.(fieldPath, { kind: 'array_remove', index: i })}
          />
        ) : undefined
        const itemDragProps = canCollectionEdit ? {
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
            setDragIdx(null)
            setOverIdx(null)
          },
        } : undefined
        if (item.kind === 'object') {
          return (
            <ArrayObjectItem
              key={i}
              index={i}
              value={item}
              depth={depth}
              fieldPath={itemPath}
              pathKey={itemPathKey}
              onEdit={onEdit}
              onCollectionEdit={onCollectionEdit}
              onRowToggle={onRowToggle}
              valueAnnotation={itemAnnotation}
              leading={dragHandle}
              trailing={trailing}
              dragProps={itemDragProps}
            />
          )
        }
        return (
          <FieldRow
            key={i}
            label={String(i + 1)}
            value={item}
            depth={depth}
            onEdit={onEdit}
            onCollectionEdit={onCollectionEdit}
            fieldPath={itemPath}
            pathKey={itemPathKey}
            onRowToggle={onRowToggle}
            declaredType={annotationDeclaredType(itemAnnotation)}
            refTargetType={annotationRefTargetType(itemAnnotation)}
            enumType={annotationEnumType(itemAnnotation)}
            nullable={annotationNullable(itemAnnotation)}
            valueAnnotation={itemAnnotation}
            leading={dragHandle}
            trailing={trailing}
            collectionItem
            dragProps={itemDragProps}
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
    ><Icon name="close" size={10} /></button>
  )
}

function CollectionAddControl({ container, depth, fieldPath, onCollectionEdit, itemAnnotation }: {
  container: FieldValue & { kind: 'array' | 'dict' }
  depth: number
  fieldPath: FieldPathSegment[]
  onCollectionEdit: (edit: CollectionEdit) => void
  itemAnnotation?: FieldAnnotation
}) {
  const [adding, setAdding] = useState(false)
  const [dupError, setDupError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const { openObjectDraft } = useObjectDraft()
  const rowSelection = useContext(ValueRowSelectionCtx)
  const pathWire = JSON.stringify(fieldPath)
  const selected = rowSelection?.selectedActionPathWire === pathWire

  function reset() { setAdding(false); setDupError(null) }

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
      <span
        className={`dc-row-add dc-collection-add${container.value.length === 0 ? ' dc-collection-add-empty' : ''}${selected ? ' keyboard-selected' : ''}`}
        data-depth={depth}
        data-add-path-wire={pathWire}
        onMouseDown={event => {
          event.stopPropagation()
          rowSelection?.onSelectAction?.(pathWire)
        }}
        onClick={event => event.stopPropagation()}
      >
        <button
          className="btn-add-item"
          title="添加元素"
          aria-label="添加元素"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              addArrayItem()
            } finally {
              setBusy(false)
            }
          }}
        >
          <Icon name="plus" size={11} />
        </button>
      </span>
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
    <span
      className={`dc-row-add dc-collection-add${container.value.length === 0 ? ' dc-collection-add-empty' : ''}${selected ? ' keyboard-selected' : ''}`}
      data-depth={depth}
      data-add-path-wire={pathWire}
      onMouseDown={event => {
        event.stopPropagation()
        rowSelection?.onSelectAction?.(pathWire)
      }}
      onClick={event => event.stopPropagation()}
    >
      {!adding ? (
        <button className="btn-add-item" title="添加项" aria-label="添加项" onClick={() => { setAdding(true); setDupError(null) }}>
          <Icon name="plus" size={11} />
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
    </span>
  )
}

function ArrayObjectItem({
  index,
  value,
  depth,
  fieldPath,
  pathKey,
  onEdit,
  onCollectionEdit,
  onRowToggle,
  valueAnnotation,
  leading,
  trailing,
  dragProps,
}: {
  index: number
  value: FieldValue & { kind: 'object' }
  depth: number
  fieldPath: FieldPathSegment[]
  pathKey: string
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  onRowToggle?: (path: string, expanded: boolean) => void
  valueAnnotation?: FieldAnnotation
  leading?: ReactNode
  trailing?: ReactNode
  dragProps?: { extraClass?: string } & Omit<React.HTMLAttributes<HTMLDivElement>, 'className'> & { draggable?: boolean }
}) {
  const diag = rowDiagSeverity(pathKey)
  const rowSelection = useContext(ValueRowSelectionCtx)
  const selected = sameFieldPath(rowSelection?.selectedFieldPath, fieldPath)
  const hasFields = objectFields(value).length > 0

  return (
    <div
      className={`dc-array-object-item${selected ? ' keyboard-selected' : ''}${diag.sev ? ` dc-array-item-diag-${diag.sev}` : ''}${dragProps?.extraClass ? ` ${dragProps.extraClass}` : ''}`}
      style={inspectorDepthStyle(depth)}
      data-depth={depth}
      data-field-path={pathKey}
      data-field-path-wire={JSON.stringify(fieldPath)}
      data-value-kind="object"
      data-keyboard-editable={!!onEdit || undefined}
      title={diag.messages.join('\n') || undefined}
      onMouseDown={() => rowSelection?.onSelectValue?.(fieldPath)}
      {...(dragProps && {
        onDragStart: dragProps.onDragStart,
        onDragOver: dragProps.onDragOver,
        onDragLeave: dragProps.onDragLeave,
        onDrop: dragProps.onDrop,
        onDragEnd: dragProps.onDragEnd,
        draggable: dragProps.draggable,
      })}
    >
      <div className="dc-array-item-rail">
        {leading}
        <span className="dc-array-item-index">{index + 1}</span>
        {(diag.sev === 'error' || diag.sev === 'warning') && (
          <span className={`dc-array-item-diag-dot ${diag.sev}`} aria-hidden />
        )}
      </div>
      <div className="dc-array-object-fields">
        {hasFields ? (
          <ComplexValueChildren
            value={value}
            depth={depth + 1}
            fieldPath={fieldPath}
            pathKey={pathKey}
            onEdit={onEdit}
            onCollectionEdit={onCollectionEdit}
            onRowToggle={onRowToggle}
            valueAnnotation={valueAnnotation}
            firstChildTrailing={trailing}
          />
        ) : (
          <div className="dc-row dc-array-empty-object" style={inspectorDepthStyle(depth + 1)}>
            <div className="dc-row-label"><span className="vc vc-null">空对象</span></div>
            <div className="dc-row-value" />
            <div className="dc-row-actions">{trailing}</div>
          </div>
        )}
      </div>
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
  const [variants, setVariants] = useState<{ name: string, label: string | null, description: string | null }[] | null>(null)
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
        <SearchableSelect
          className="dc-input"
          autoFocus
          value=""
          placeholder="选择..."
          options={variants.map(v => ({ value: v.name, label: v.label ?? v.name, description: v.description ?? undefined }))}
          onCommit={next => {
            if (next) onCommit({ kind: 'enum', value: { enum_name: sampleKey.value.enum_name, variant: next, value: 0n } })
          }}
          onExit={onCancel}
        />
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
