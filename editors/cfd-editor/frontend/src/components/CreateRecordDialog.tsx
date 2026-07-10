import { useMemo, useState } from 'react'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { FieldValue } from '../wire'
import { ObjectDraftDialog } from './ObjectDraftDialog'

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
  const trimmedKey = recordKeyDraft.trim()
  const existingKeySet = useMemo(() => new Set(existingKeys), [existingKeys])
  const duplicateKey = !!trimmedKey && existingKeySet.has(trimmedKey)

  return (
    <ObjectDraftDialog
      title="新建记录"
      actualType={actualType}
      onLoadDraft={onCreateRecordDraft}
      onConfirm={async payload => {
        if (payload.kind !== 'object') return
        await onInsertRecord(trimmedKey, payload.value.actual_type, payload)
        onClose()
      }}
      onClose={onClose}
      confirmLabel="创建"
      extraValidation={() => {
        if (!trimmedKey) return 'Key 不能为空'
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
          aria-invalid={!trimmedKey || duplicateKey}
          title={duplicateKey ? `Key "${trimmedKey}" 已存在` : undefined}
          onChange={e => setRecordKeyDraft(e.target.value)}
        />
      )}
    />
  )
}
