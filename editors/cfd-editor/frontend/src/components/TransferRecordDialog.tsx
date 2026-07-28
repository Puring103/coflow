import { useMemo, useState } from 'react'
import { Icon } from './Icon'

export interface RecordTransferTarget {
  filePath: string
  recordCount: number
}

interface Props {
  recordKey: string
  targets: RecordTransferTarget[]
  onConfirm: (destinationFile: string, targetIndex: number) => Promise<void>
  onClose: () => void
}

export function TransferRecordDialog({ recordKey, targets, onConfirm, onClose }: Props) {
  const [destinationFile, setDestinationFile] = useState(targets[0]?.filePath ?? '')
  const target = useMemo(
    () => targets.find(candidate => candidate.filePath === destinationFile) ?? targets[0],
    [destinationFile, targets],
  )
  const [targetIndex, setTargetIndex] = useState(target?.recordCount ?? 0)
  const [saving, setSaving] = useState(false)
  const valid = !!target
    && Number.isInteger(targetIndex)
    && targetIndex >= 0
    && targetIndex <= target.recordCount

  const submit = async () => {
    if (!target || !valid || saving) return
    setSaving(true)
    try {
      await onConfirm(target.filePath, targetIndex)
      onClose()
    } finally {
      setSaving(false)
    }
  }

  return (
    <div
      className="create-record-backdrop"
      role="presentation"
      onMouseDown={event => { if (event.target === event.currentTarget) onClose() }}
    >
      <section
        className="create-record-dialog transfer-record-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="移动记录到其他文件"
        onMouseDown={event => event.stopPropagation()}
        onKeyDown={event => {
          if (event.key === 'Escape') onClose()
          if (event.key === 'Enter') void submit()
        }}
      >
        <div className="create-record-card-header">
          <div className="gn-color-bar" />
          <strong className="transfer-record-title">移动 {recordKey}</strong>
          <button className="btn-icon create-record-close" onClick={onClose} aria-label="关闭移动记录">
            <Icon name="close" size={14} />
          </button>
        </div>
        <div className="transfer-record-fields">
          <label>
            <span>目标文件</span>
            <select
              value={target?.filePath ?? ''}
              autoFocus
              onChange={event => {
                const filePath = event.target.value
                const next = targets.find(candidate => candidate.filePath === filePath)
                setDestinationFile(filePath)
                setTargetIndex(next?.recordCount ?? 0)
              }}
            >
              {targets.map(candidate => (
                <option key={candidate.filePath} value={candidate.filePath}>
                  {candidate.filePath}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>目标位置</span>
            <input
              type="number"
              min={0}
              max={target?.recordCount ?? 0}
              step={1}
              value={targetIndex}
              onChange={event => setTargetIndex(event.target.valueAsNumber)}
            />
          </label>
        </div>
        <footer className="create-record-actions">
          <button className="btn" onClick={onClose} disabled={saving}>取消</button>
          <button className="btn btn-primary" onClick={() => void submit()} disabled={!valid || saving}>
            {saving ? '移动中...' : '移动'}
          </button>
        </footer>
      </section>
    </div>
  )
}
