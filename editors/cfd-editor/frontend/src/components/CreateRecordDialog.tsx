import { useMemo, useState } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { FieldValue } from '../wire'
import { ObjectDraftDialog } from './ObjectDraftDialog'

interface Props {
  actualType: string
  typeOptions: string[]
  existingIds: string[]
  initialId?: string
  onCreateRecordDraft: (actualType: string) => Promise<CreateRecordDraft>
  onInsertRecord: (recordId: string, actualType: string, fields: FieldValue) => Promise<void>
  onClose: () => void
}

export function CreateRecordDialog({
  actualType,
  typeOptions,
  existingIds,
  initialId = '',
  onCreateRecordDraft,
  onInsertRecord,
  onClose,
}: Props) {
  const [selectedType, setSelectedType] = useState(actualType)
  const [recordIdDraft, setRecordIdDraft] = useState(initialId)
  const [idTouched, setIdTouched] = useState(false)
  const trimmedId = recordIdDraft.trim()
  const existingIdSet = useMemo(() => new Set(existingIds), [existingIds])
  const duplicateId = !!trimmedId && existingIdSet.has(trimmedId)

  return (
    <ObjectDraftDialog
      title="新建记录"
      actualType={selectedType}
      polymorphicTypes={typeOptions}
      onTypeChange={setSelectedType}
      alwaysShowTypeSelect
      onLoadDraft={onCreateRecordDraft}
      onConfirm={async payload => {
        if (payload.kind !== 'object') return
        await onInsertRecord(trimmedId, payload.value.actual_type, payload)
        onClose()
      }}
      onClose={onClose}
      confirmLabel="创建"
      extraValidation={() => {
        // ID 错误由输入框状态表达，空白字符串仅用于阻止提交。
        if (!trimmedId || duplicateId) return ' '
        return null
      }}
      headerExtras={(
        <input
          className="create-record-key-input"
          value={recordIdDraft}
          autoFocus
          placeholder="请输入记录 ID"
          aria-label="记录 ID"
          aria-invalid={(idTouched && !trimmedId) || duplicateId}
          title={duplicateId ? `ID "${trimmedId}" 已存在于该类型的继承域中` : undefined}
          onChange={e => {
            setIdTouched(true)
            setRecordIdDraft(e.target.value)
          }}
          onBlur={() => setIdTouched(true)}
        />
      )}
    />
  )
}
