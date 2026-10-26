import { cellMotionOrigin, type MotionOrigin } from '../../../shared/motion/motion'
import { useTableExpansion } from './tableExpansion'

import { observeCellSize } from './cellSizeObserver'

import { useState, useEffect, useMemo, useRef, memo, createContext, useContext } from 'react'

import type { Cell } from '@tanstack/react-table'

import type { RecordCoordinate } from '../../../bindings/RecordCoordinate'
import type { RecordRow } from '../../../bindings/RecordRow'

import { coordinateId, cloneValue, isComplexValue, presentationValue, replacePresentationValue, type FieldPathSegment, type FieldValue } from '../../../wire'
import { DataCardCompact, EnumDirectSelect, RefDirectSelect } from '../../record-editing/components/DataCard'

import { Icon } from '../../../shared/ui/Icon'
import { ScalarValueInput, ScalarValuePreview } from '../../record-editing/components/ScalarValueInput'

import { PluginContributionMount, currentPluginIdentity, usePluginPresentation } from '../../plugins/runtime'
import type { PluginPresentationContext } from '../../plugins/runtime/types'

// 单元格组件身份稳定，列快照刷新时保留输入与草稿。
export function CellSyntaxEditor({
  initialText,
  onCommit,
  onCancel,
}: {
  initialText: string
  onCommit: (text: string) => Promise<void>
  onCancel: () => void
}) {
  const [text, setText] = useState(initialText)
  const [busy, setBusy] = useState(false)
  return (
    <input
      className="dc-input dc-input-flat cell-syntax-editor"
      value={text}
      autoFocus
      readOnly={busy}
      onChange={e => setText(e.target.value)}
      onBlur={onCancel}
      onKeyDown={async e => {
        e.stopPropagation()
        if (e.key === 'Escape') onCancel()
        if (e.key === 'Enter' && !busy) {
          e.preventDefault()
          setBusy(true)
          await onCommit(text)
          setBusy(false)
        }
      }}
      aria-label="单元格语法编辑器"
    />
  )
}

interface EditableCellProps {
  diffChangedPaths?: ReadonlySet<string>
  onExpand?: (origin: MotionOrigin) => void
  expandedPaths?: ReadonlySet<string>
  onRowToggle?: (path: string, expanded: boolean) => void
  onEdit?: (path: FieldPathSegment[], next: FieldValue) => void
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  value: FieldValue
  editable: boolean
  annotation?: RecordRow['fields'][number]['annotation']
  refTargetType?: string
  enumType?: string
  enumIsFlag?: boolean
  nullable?: boolean
  declaredType?: string
  highlightQuery?: string
  onCommit?: (next: FieldValue) => void
  onEditingFinished?: () => void
}

// 列定义随快照刷新时渲染函数身份会变化；固定组件边界才能保留输入节点、光标和草稿。
export const TablePreviewRows = createContext<number | undefined>(undefined)
export const TableCellContent = memo(function TableCellContent({ cell }: {
  cell: Cell<RecordRow, unknown>
  diagnostics: unknown
  query: string
  syntaxEdit: unknown
  rowPresentation: unknown
  writeField: unknown
  renameRecord: unknown
  diagnosticClick: unknown
  expandCell: unknown
}) {
  const render = cell.column.columnDef.cell
  return typeof render === 'function' ? render(cell.getContext()) : render ?? null
})

export function EditableCell(props: EditableCellProps) {
  const { filePath, coordinate, fieldPath, value, declaredType } = props
  const viewportRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [overflow, setOverflow] = useState(false)
  useEffect(() => {
    const viewport = viewportRef.current, content = contentRef.current
    if (!viewport || !content) return
    return observeCellSize([viewport, content], () => setOverflow(content.scrollHeight > viewport.clientHeight + 1))
  }, [value])
  const identity = currentPluginIdentity()
  const presentation = usePluginPresentation('cell', declaredType ?? '')
  const context = useMemo<PluginPresentationContext | null>(() => (
    identity && presentation
      ? {
          identity,
          filePath,
          coordinate: { ...coordinate },
          actualType: coordinate.actual_type,
          fieldPath: fieldPath.map(segment => ({ ...segment })),
          declaredType: declaredType ?? '',
          value: cloneValue(value),
        }
      : null
  ), [identity?.sessionId, identity?.revision, filePath, coordinate.actual_type, coordinate.id, declaredType, value, presentation])
  // 内置字段树与插件共用同一预览边界，放大入口不依赖具体渲染器。
  const content = presentation?.slot === 'cell' && context
    ? <PluginContributionMount contribution={presentation} context={context} inline className="plugin-cell-presentation" />
    : <EditableCellBuiltIn {...props} />
  if (!isComplexValue(presentationValue(value))) return content
  return <div className={`cell-complex-preview${overflow ? ' overflowing' : ''}`}
    onDoubleClick={props.onExpand ? event => {
      if ((event.target as HTMLElement).closest('button, input, textarea, select, a, .dc-row-foldout, .dc-row-field, [contenteditable="true"]')) return
      event.stopPropagation()
      props.onExpand?.(cellMotionOrigin(event.currentTarget))
    } : undefined}>
    <div ref={viewportRef} className="cell-complex-viewport"><div ref={contentRef} className="cell-complex-content">{content}</div></div>
    {overflow && <span className="cell-complex-overflow" aria-hidden>…</span>}
    {props.onExpand && <button type="button" className="cell-expand-button" aria-label="放大编辑单元格" title="放大编辑（也可双击空白处）" onMouseDown={event => event.stopPropagation()} onClick={event => { event.stopPropagation(); props.onExpand?.(cellMotionOrigin(event.currentTarget)) }}><Icon name="expand" size={11}/></button>}
  </div>
}

function EditableCellBuiltIn({
  coordinate, value, editable, annotation, refTargetType, enumType, enumIsFlag, nullable, highlightQuery, onCommit, onEditingFinished, fieldPath, onEdit, expandedPaths, onRowToggle, diffChangedPaths,
}: EditableCellProps) {
  const previewRows = useContext(TablePreviewRows)
  const expansion = useTableExpansion(coordinateId(coordinate))
  const shownValue = presentationValue(value)
  const commitValue = onCommit
    ? (next: FieldValue) => onCommit(replacePresentationValue(value, next))
    : undefined
  const isScalar = shownValue.kind === 'bool' || shownValue.kind === 'int' || shownValue.kind === 'float'
                || shownValue.kind === 'string' || shownValue.kind === 'formatted_string'
                || shownValue.kind === 'enum' || shownValue.kind === 'ref'
  // null cells become editable when the schema tells us they hold an enum/ref/bool
  const isNullDropdown = shownValue.kind === 'option_none' && !!(enumType || refTargetType)
  const canEdit = editable && (isScalar || isNullDropdown) && !!commitValue
  const commitAndRestoreFocus = (next: FieldValue) => {
    commitValue!(next)
    requestAnimationFrame(() => onEditingFinished?.())
  }

  if (canEdit && (shownValue.kind === 'string' || shownValue.kind === 'formatted_string' || shownValue.kind === 'int' || shownValue.kind === 'float')) {
    return <ScalarValueInput value={shownValue} onCommit={commitValue!} />
  }

  if (!canEdit && (shownValue.kind === 'string' || shownValue.kind === 'formatted_string' || shownValue.kind === 'int' || shownValue.kind === 'float')) {
    return <ScalarValuePreview value={shownValue} />
  }

  // Bool: checkbox, always visible
  if (canEdit && shownValue.kind === 'bool') {
    return (
      <div className="cell-edit-wrap">
        <input
          type="checkbox"
          className="dc-checkbox"
          checked={shownValue.value}
          onChange={e => commitValue!({ kind: 'bool', value: e.target.checked })}
        />
      </div>
    )
  }

  // Enum
  if (canEdit && (shownValue.kind === 'enum' || (shownValue.kind === 'option_none' && enumType))) {
    return (
      <div className="cell-edit-wrap">
        <EnumDirectSelect
          value={shownValue as FieldValue & { kind: 'enum' | 'option_none' }}
          enumType={enumType}
          isFlag={enumIsFlag}
          nullable={nullable}
          onCommit={enumIsFlag ? commitValue! : commitAndRestoreFocus}
          onExit={onEditingFinished}
        />
      </div>
    )
  }

  // Ref
  if (canEdit && (shownValue.kind === 'ref' || (shownValue.kind === 'option_none' && refTargetType))) {
    return (
      <div className="cell-edit-wrap">
        <RefDirectSelect
          value={shownValue as FieldValue & { kind: 'ref' | 'option_none' }}
          onCommit={commitAndRestoreFocus}
          targetType={refTargetType}
          nullable={nullable}
          onExit={onEditingFinished}
        />
      </div>
    )
  }

  return (
    <div className={`cell-edit-wrap${canEdit ? ' editable' : ''}`}>
      <DataCardCompact
        diffChangedPaths={diffChangedPaths}
        previewRows={previewRows}
        value={shownValue}
        fieldPath={fieldPath}
        pathPrefix={fieldPath.map(segment => String(segment.value)).join('.')}
        expandedPaths={expandedPaths ?? expansion.paths}
        onRowToggle={onRowToggle ?? expansion.toggle}
        onEdit={editable ? onEdit : undefined}
        annotation={annotation}
        refTargetType={refTargetType}
        highlightQuery={highlightQuery}
      />
    </div>
  )
}
