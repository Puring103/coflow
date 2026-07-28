import { useRef, useState } from 'react'
import type { RecordRow } from '../bindings/RecordRow'
import {
  cellDeclaredType,
  cellEnumType,
  cellNullable,
  cellRefTargetType,
  fieldPathField,
  nullValue,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { projectBatchRecordFields } from '../state/batchRecordProjection'
import { DataCardExpanded, EnumDirectSelect, RefDirectSelect } from './DataCard'
import { fieldTypeColor } from '../utils/typeColor'

interface Props {
  records: readonly RecordRow[]
  readOnly?: boolean
  onWriteFields?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<void>
}

export function BatchRecordEditor({ records, readOnly, onWriteFields }: Props) {
  const fields = projectBatchRecordFields(records)
  const [busyField, setBusyField] = useState<string | null>(null)

  async function commit(name: string, path: FieldPathSegment[], value: FieldValue) {
    if (!onWriteFields || busyField) return
    setBusyField(name)
    try {
      await onWriteFields(path, value)
    } finally {
      setBusyField(null)
    }
  }

  return (
    <div className="batch-record-editor">
      <div className="batch-record-header">
        <strong>已选择 {records.length} 条记录</strong>
        <span>{fields.length} 个公共字段</span>
      </div>
      {fields.length === 0 ? (
        <div className="empty-hint">所选记录没有类型兼容的公共字段</div>
      ) : fields.map(field => {
        const path = [fieldPathField(field.cell.name)]
        const editable = !readOnly && field.editable && !!onWriteFields && !busyField
        if (field.state === 'same') {
          return (
            <DataCardExpanded
              key={field.cell.name}
              fields={[field.cell]}
              actualType={records[0]?.coordinate.actual_type}
              onEdit={editable ? (nextPath, value) => { void commit(field.cell.name, nextPath, value) } : undefined}
            />
          )
        }
        return (
          <MixedFieldRow
            key={field.cell.name}
            label={field.cell.name}
            sample={field.cell.value}
            declaredType={cellDeclaredType(field.cell)}
            enumType={cellEnumType(field.cell)}
            refTargetType={cellRefTargetType(field.cell)}
            nullable={cellNullable(field.cell)}
            disabled={!editable}
            busy={busyField === field.cell.name}
            onCommit={value => { void commit(field.cell.name, path, value) }}
          />
        )
      })}
    </div>
  )
}

export function MixedFieldRow({ label, sample, declaredType, enumType, refTargetType, nullable, disabled, busy, onCommit }: {
  label: string
  sample: FieldValue
  declaredType?: string
  enumType?: string
  refTargetType?: string
  nullable: boolean
  disabled: boolean
  busy: boolean
  onCommit: (value: FieldValue) => void
}) {
  const valueKind = mixedValueKind(sample, declaredType, enumType, refTargetType)
  return (
    <div className={`dc-row batch-mixed-row${disabled ? ' disabled' : ''}`} data-value-kind="mixed">
      <div className="dc-row-label">
        <span className="dc-row-label-text">{label}</span>
        {declaredType && (
          <span className="dc-field-type-hint" style={{ '--field-color': fieldTypeColor(declaredType) } as React.CSSProperties}>
            {declaredType}
          </span>
        )}
      </div>
      <div className="dc-row-value">
        <div className="dc-row-value-inner">
          {busy ? <span className="batch-mixed-value">保存中...</span> : disabled ? (
            <span className="batch-mixed-value">...</span>
          ) : valueKind === 'bool' ? (
            <MixedCheckbox onCommit={onCommit} />
          ) : valueKind === 'enum' && enumType ? (
            <EnumDirectSelect value={nullValue() as FieldValue & { kind: 'null' }} enumType={enumType} nullable={nullable} onCommit={onCommit} />
          ) : valueKind === 'ref' && refTargetType ? (
            <RefDirectSelect value={nullValue() as FieldValue & { kind: 'null' }} targetType={refTargetType} nullable={nullable} onCommit={onCommit} />
          ) : valueKind === 'int' || valueKind === 'float' || valueKind === 'string' ? (
            <MixedTextInput kind={valueKind} declaredType={declaredType} onCommit={onCommit} />
          ) : (
            <span className="batch-mixed-value" title="不同的集合或对象不能直接批量合并">...</span>
          )}
        </div>
      </div>
    </div>
  )
}

function MixedCheckbox({ onCommit }: { onCommit: (value: FieldValue) => void }) {
  const ref = useRef<HTMLInputElement>(null)
  return (
    <input
      ref={element => {
        ref.current = element
        if (element) element.indeterminate = true
      }}
      type="checkbox"
      className="dc-checkbox"
      aria-label="多个布尔值"
      onChange={event => onCommit({ kind: 'bool', value: event.target.checked })}
    />
  )
}

function MixedTextInput({ kind, declaredType, onCommit }: {
  kind: 'int' | 'float' | 'string'
  declaredType?: string
  onCommit: (value: FieldValue) => void
}) {
  const [text, setText] = useState('')
  const [dirty, setDirty] = useState(false)
  function commit() {
    if (!dirty) return
    try {
      if (kind === 'int') {
        if (!text) {
          setDirty(false)
          return
        }
        onCommit({ kind: 'int', value: BigInt(text) })
      }
      else if (kind === 'float') {
        const value = Number(text)
        if (text && Number.isFinite(value)) onCommit({ kind: 'float', value })
      } else onCommit({ kind: 'string', value: text })
      setText('')
      setDirty(false)
    } catch {
      setText('')
      setDirty(false)
    }
  }
  return (
    <input
      className="dc-input dc-input-flat dc-input-themed batch-mixed-input"
      style={{ '--field-color': fieldTypeColor(declaredType ?? kind) } as React.CSSProperties}
      type={kind === 'string' ? 'text' : 'number'}
      value={text}
      placeholder="..."
      onChange={event => {
        setText(event.target.value)
        setDirty(true)
      }}
      onBlur={commit}
      onKeyDown={event => {
        if (event.key === 'Enter') event.currentTarget.blur()
        if (event.key === 'Escape') {
          setText('')
          setDirty(false)
          event.currentTarget.blur()
        }
      }}
    />
  )
}

function mixedValueKind(
  sample: FieldValue,
  declaredType?: string,
  enumType?: string,
  refTargetType?: string,
): FieldValue['kind'] {
  if (enumType) return 'enum'
  if (refTargetType) return 'ref'
  if (sample.kind !== 'null') return sample.kind
  const normalized = declaredType?.replace(/\?$/, '').toLowerCase()
  if (normalized === 'bool') return 'bool'
  if (normalized?.startsWith('int') || normalized?.startsWith('uint')) return 'int'
  if (normalized === 'float' || normalized === 'double' || normalized === 'number') return 'float'
  if (normalized === 'string') return 'string'
  return 'null'
}
