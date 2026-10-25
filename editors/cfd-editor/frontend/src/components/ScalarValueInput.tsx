import { DataFillContext } from '../state/dataFillContext'
import { registerDataEdit } from '../state/editSession'
import { useContext, useEffect, useRef, useState } from 'react'
import type { FieldValue } from '../wire'
import { errorMessage } from '../wire'
import { parseFieldValueText, plainFieldValueText } from '../value/fieldValue'
import { RichTextInput } from './RichTextInput'

/** 表格、维度和详情共用直接输入；失焦保存不抢回用户刚点击的焦点。 */
export function ScalarValueInput({ value, onCommit, label, autoFocus = false }: {
  value: FieldValue & { kind: 'string' | 'formatted_string' | 'int' | 'float' }
  onCommit: (next: FieldValue) => unknown
  label?: string
  autoFocus?: boolean
}) {
  const fillSelection = useContext(DataFillContext)
  const incoming = plainFieldValueText(value)
  const [text, setText] = useState(incoming)
  const [error, setError] = useState<string | null>(null)
  const dirty = useRef(false)
  const saved = useRef(incoming)
  const draft = useRef(text)
  const writeSequence = useRef(0)
  const pending = useRef<string | null>(null)
  const element = useRef<HTMLSpanElement>(null)
  const inFlight = useRef<Promise<boolean> | null>(null)
  const commitRef = useRef<() => Promise<boolean>>(async () => true)
  useEffect(() => registerDataEdit(() => commitRef.current(), () => dirty.current || inFlight.current !== null), [])

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

  const commit = (fill = false): Promise<boolean> => {
    if (!fill && (!dirty.current || draft.current === saved.current)) return inFlight.current ?? Promise.resolve(true)
    const task = submit(fill)
    inFlight.current = task
    void task.finally(() => { if (inFlight.current === task) inFlight.current = null })
    return task
  }
  const submit = async (fill: boolean): Promise<boolean> => {
    const submitted = draft.current
    const next = parseFieldValueText(value, submitted)
    if (!next) { setError('输入值不符合字段类型'); return false }
    const sequence = ++writeSequence.current
    dirty.current = false
    saved.current = submitted
    pending.current = submitted
    try {
      if (fill && fillSelection) await fillSelection(next)
      else await onCommit(next)
      if (sequence === writeSequence.current) setError(null)
      return true
    } catch (cause) {
      if (sequence !== writeSequence.current) return false
      dirty.current = true
      pending.current = null
      saved.current = incoming
      setError(errorMessage(cause))
      element.current?.querySelector('textarea')?.focus({ preventScroll: true })
      return false
    }
  }

  commitRef.current = commit
  const exit = (tab?: boolean) => {
    const input = element.current?.querySelector('textarea')
    const root = input?.closest<HTMLElement>('.table-scroll, .inspector-body, .rv-main, .dimension-table-scroll')
    input?.blur()
    root?.focus({ preventScroll: true })
    if (tab !== undefined) root?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: tab, bubbles: true, cancelable: true }))
  }
  return <span ref={element} className="string-value-input" title={error ?? undefined}>
    <RichTextInput cell rows={1} className="cell-string-textarea" value={text}
      aria-label={label ?? (value.kind === 'int' || value.kind === 'float' ? '数值' : '字符串')} aria-invalid={!!error} autoFocus={autoFocus}
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
          exit()
        } else if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
          event.preventDefault()
          event.stopPropagation()
          const tab = event.key === 'Tab' ? event.shiftKey : undefined
          const submitted = draft.current
          const input = event.currentTarget
          void commit(event.key === 'Enter' && (event.ctrlKey || event.metaKey)).then(ok => { if (ok && draft.current === submitted && document.activeElement === input) exit(tab) })
        }
      }} />
    {error && <span className="string-value-error" role="alert">{error}</span>}
  </span>
}
