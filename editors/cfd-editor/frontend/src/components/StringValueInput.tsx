import { useEffect, useRef, useState } from 'react'
import type { FieldValue } from '../wire'
import { errorMessage } from '../wire'
import { parseFieldValueText, plainFieldValueText } from '../value/fieldValue'
import { RichTextInput } from './RichTextInput'

/** 表格、维度和详情共用直接输入；失焦保存不抢回用户刚点击的焦点。 */
export function StringValueInput({ value, onCommit, label = '字符串', autoFocus = false }: {
  value: FieldValue & { kind: 'string' | 'formatted_string' }
  onCommit: (next: FieldValue) => unknown
  label?: string
  autoFocus?: boolean
}) {
  const incoming = plainFieldValueText(value)
  const [text, setText] = useState(incoming)
  const [error, setError] = useState<string | null>(null)
  const dirty = useRef(false)
  const saved = useRef(incoming)
  const draft = useRef(text)
  const writeSequence = useRef(0)
  const pending = useRef<string | null>(null)

  useEffect(() => {
    // 异步保存返回旧快照时不能覆盖正在继续输入的草稿。
    if (pending.current !== null && incoming !== pending.current) return
    pending.current = null
    saved.current = incoming
    if (!dirty.current) {
      draft.current = incoming
      setText(incoming)
    }
  }, [incoming])

  const commit = async () => {
    if (!dirty.current || draft.current === saved.current) return
    const submitted = draft.current
    const next = parseFieldValueText(value, submitted)
    if (!next) return
    const sequence = ++writeSequence.current
    dirty.current = false
    saved.current = submitted
    pending.current = submitted
    try {
      await onCommit(next)
      if (sequence === writeSequence.current) setError(null)
    } catch (cause) {
      if (sequence !== writeSequence.current) return
      dirty.current = true
      pending.current = null
      saved.current = incoming
      setError(errorMessage(cause))
    }
  }

  return <span className="string-value-input" title={error ?? undefined}>
    <RichTextInput cell rows={1} className="cell-string-textarea" value={text}
      aria-label={label} aria-invalid={!!error} autoFocus={autoFocus}
      onValueChange={next => {
        draft.current = next
        dirty.current = true
        setText(next)
        setError(null)
      }}
      onBlur={() => { void commit() }}
      onKeyDown={event => {
        if (event.key === 'Escape') {
          event.preventDefault()
          event.stopPropagation()
          dirty.current = false
          draft.current = saved.current
          setText(saved.current)
          setError(null)
        } else if (event.key === 'Enter' && !event.shiftKey) {
          event.preventDefault()
          void commit()
        }
      }} />
    {error && <span className="string-value-error" role="alert">{error}</span>}
  </span>
}
