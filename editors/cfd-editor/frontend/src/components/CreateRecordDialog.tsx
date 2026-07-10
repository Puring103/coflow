import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { CreateRecordFieldDraft } from '../bindings/CreateRecordFieldDraft'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { FieldCell } from '../bindings/FieldCell'
import type { FieldValue } from '../wire'
import { DataCardExpanded } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

interface Props {
  actualType: string
  existingKeys: string[]
  initialKey?: string
  onCreateRecordDraft: (actualType: string) => Promise<CreateRecordDraft>
  onInsertRecord: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  onClose: () => void
}

export function CreateRecordDialog({
  actualType,
  existingKeys,
  initialKey = '',
  onCreateRecordDraft,
  onInsertRecord,
  onClose,
}: Props) {
  const [recordKeyDraft, setRecordKeyDraft] = useState(initialKey)
  const [draft, setDraft] = useState<CreateRecordDraft | null>(null)
  const [values, setValues] = useState<Record<string, FieldValue | null>>({})
  const [dirty, setDirty] = useState<Set<string>>(() => new Set())
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!actualType) return
    let alive = true
    setLoading(true)
    setLoadError(null)
    setDraft(null)
    setValues({})
    setDirty(new Set())
    onCreateRecordDraft(actualType)
      .then(next => {
        if (!alive) return
        setDraft(next)
        setValues(Object.fromEntries(next.fields.map(field => [field.name, field.value])))
      })
      .catch(err => {
        if (!alive) return
        setLoadError(errorText(err))
      })
      .finally(() => { if (alive) setLoading(false) })
    return () => { alive = false }
  }, [actualType, onCreateRecordDraft])

  const requiredFields = useMemo(
    () => draft?.fields.filter(field => isRequiredMissing(field, values[field.name] ?? null)) ?? [],
    [draft, values],
  )
  const trimmedKey = recordKeyDraft.trim()
  const existingKeySet = useMemo(() => new Set(existingKeys), [existingKeys])
  const duplicateKey = !!trimmedKey && existingKeySet.has(trimmedKey)
  const canSubmit = !!trimmedKey && !duplicateKey && !!draft && requiredFields.length === 0 && !saving && !loading

  function setFieldValue(fieldName: string, next: FieldValue) {
    setValues(prev => ({ ...prev, [fieldName]: next }))
    setDirty(prev => {
      const out = new Set(prev)
      out.add(fieldName)
      return out
    })
  }

  async function submit() {
    if (!canSubmit || !draft) return
    setSaving(true)
    try {
      await onInsertRecord(trimmedKey, draft.actual_type, buildInsertPayload(draft, values, dirty))
      onClose()
    } finally {
      setSaving(false)
    }
  }

  const typeColorValue = typeColor(actualType)

  return (
    <div
      className="create-record-backdrop"
      role="presentation"
      onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}
    >
      <section
        className="create-record-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="新建记录"
        onMouseDown={e => e.stopPropagation()}
        onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      >
        <div className="create-record-card-header" style={{ '--node-color': typeColorValue } as CSSProperties}>
          <div className="gn-color-bar" />
          <input
            className="create-record-key-input"
            value={recordKeyDraft}
            autoFocus
            placeholder="record_key"
            aria-label="记录 Key"
            aria-invalid={!trimmedKey || duplicateKey}
            title={duplicateKey ? `Key "${trimmedKey}" 已存在` : undefined}
            onChange={e => setRecordKeyDraft(e.target.value)}
          />
          <span className="create-record-type-tag">{actualType}</span>
          <button className="btn-icon create-record-close" onClick={onClose} aria-label="关闭新建记录表单">
            <Icon name="close" size={14} />
          </button>
        </div>

        {duplicateKey && (
          <div className="create-record-error" role="alert">
            Key "{trimmedKey}" 已存在于该类型的继承域中，请换一个 Key。
          </div>
        )}
        {loadError && <div className="create-record-error" role="alert">{loadError}</div>}
        {loading && <div className="create-record-loading">正在读取字段默认值...</div>}

        {draft && (
          <CreateRecordFieldsBody
            draft={draft}
            values={values}
            dirty={dirty}
            onEditField={setFieldValue}
          />
        )}

        <footer className="create-record-actions">
          <button className="btn" onClick={onClose} disabled={saving}>取消</button>
          <button className="btn btn-primary" onClick={submit} disabled={!canSubmit}>
            {saving ? '创建中...' : '创建'}
          </button>
        </footer>
      </section>
    </div>
  )
}

function CreateRecordFieldsBody({
  draft,
  values,
  dirty,
  onEditField,
}: {
  draft: CreateRecordDraft
  values: Record<string, FieldValue | null>
  dirty: Set<string>
  onEditField: (fieldName: string, next: FieldValue) => void
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const fieldsForCard: FieldCell[] = useMemo(
    () => draft.fields.map(field => ({
      name: field.name,
      value: fieldValueForDraft(values[field.name] ?? null),
      annotation: annotationForDraft(field),
    })),
    [draft, values],
  )

  // Reuses .dc-row visuals; the flags for schema-default muting / required
  // marker aren't part of the shared card, so mark rows via a post-render DOM
  // pass keyed to the same data-field-name attribute DataCardExpanded emits.
  useLayoutEffect(() => {
    const root = containerRef.current
    if (!root) return
    for (const field of draft.fields) {
      const row = root.querySelector<HTMLElement>(
        `.dc-row[data-field-name="${cssEscape(field.name)}"]`,
      )
      if (!row) continue
      row.classList.toggle(
        'create-field-muted',
        field.source === 'schema_default' && !dirty.has(field.name),
      )
      row.classList.toggle('create-field-required', !!field.required)
    }
  }, [draft, dirty, values])

  return (
    <div className="create-record-fields" ref={containerRef}>
      <DataCardExpanded
        fields={fieldsForCard}
        actualType={draft.actual_type}
        onEdit={(path, next) => {
          const first = path[0]
          if (!first || first.kind !== 'field') return
          onEditField(first.value, next)
        }}
      />
    </div>
  )
}

function fieldValueForDraft(value: FieldValue | null): FieldValue {
  if (value) return value
  return { kind: 'null' }
}

function annotationForDraft(field: CreateRecordFieldDraft): FieldAnnotation | null {
  const requiredRef = requiredRefTarget(field)
  const base = field.annotation
  if (!base && !requiredRef) return null
  return {
    spread_info: base?.spread_info ?? null,
    ref_target_file: base?.ref_target_file ?? null,
    enum_int_value: base?.enum_int_value ?? null,
    declared_type: base?.declared_type ?? null,
    ref_target_type: base?.ref_target_type ?? requiredRef ?? null,
    enum_type: base?.enum_type ?? null,
    nullable: base?.nullable ?? false,
    read_only: base?.read_only ?? false,
    item_annotation: base?.item_annotation ?? null,
    children: base?.children ?? {},
  }
}

function cssEscape(value: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(value)
  return value.replace(/["\\]/g, '\\$&')
}

function buildInsertPayload(
  draft: CreateRecordDraft,
  values: Record<string, FieldValue | null>,
  dirty: Set<string>,
): FieldValue {
  const fields: { [key: string]: FieldValue | undefined } = {}
  for (const field of draft.fields) {
    const value = values[field.name]
    if (!value) continue
    const shouldWrite = field.source !== 'schema_default' || dirty.has(field.name)
    if (shouldWrite) fields[field.name] = value
  }
  return {
    kind: 'object',
    value: {
      actual_type: draft.actual_type,
      fields,
    },
  }
}

function isRequiredMissing(field: CreateRecordFieldDraft, value: FieldValue | null): boolean {
  if (!field.required) return false
  if (field.required.kind === 'ref') return value?.kind !== 'ref' || !value.value.trim()
  return true
}

function requiredRefTarget(field: CreateRecordFieldDraft): string | undefined {
  return field.required?.kind === 'ref' ? field.required.target_type : undefined
}

function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try {
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}
