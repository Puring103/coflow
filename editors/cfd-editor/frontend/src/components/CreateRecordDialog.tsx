import { useMemo, useState } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { FieldValue } from '../wire'
import { ObjectDraftDialog } from './ObjectDraftDialog'

interface Props {
  actualType: string
  typeOptions: string[]
  existingKeys: string[]
  initialKey?: string
  onCreateRecordDraft: (actualType: string) => Promise<CreateRecordDraft>
  onInsertRecord: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  onClose: () => void
}

export function CreateRecordDialog({
  actualType,
  typeOptions,
  existingKeys,
  initialKey = '',
  onCreateRecordDraft,
  onInsertRecord,
  onClose,
}: Props) {
  const [selectedType, setSelectedType] = useState(actualType)
  const [recordKeyDraft, setRecordKeyDraft] = useState(initialKey)
  const [keyTouched, setKeyTouched] = useState(false)
  const trimmedKey = recordKeyDraft.trim()
  const existingKeySet = useMemo(() => new Set(existingKeys), [existingKeys])
  const duplicateKey = !!trimmedKey && existingKeySet.has(trimmedKey)

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
        await onInsertRecord(trimmedKey, payload.value.actual_type, payload)
        onClose()
      }}
      onClose={onClose}
      confirmLabel="创建"
      extraValidation={() => {
        if (!trimmedKey) return keyTouched ? '请输入记录 Key' : ' '
        if (duplicateKey) return `Key "${trimmedKey}" 已存在于该类型的继承域中，请换一个 Key。`
        return null
      }}
      headerExtras={(
        <input
          className="create-record-key-input"
          value={recordKeyDraft}
          autoFocus
          placeholder="record_key"
          aria-label="记录 Key"
          aria-invalid={(keyTouched && !trimmedKey) || duplicateKey}
          title={duplicateKey ? `Key "${trimmedKey}" 已存在` : undefined}
          onChange={e => {
            setKeyTouched(true)
            setRecordKeyDraft(e.target.value)
          }}
          onBlur={() => setKeyTouched(true)}
        />
      )}
    />
  )
}
