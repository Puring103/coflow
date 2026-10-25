import {
  useMemo,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type TextareaHTMLAttributes,
} from 'react'
import { createPortal } from 'react-dom'

type RichTextFormat = 'HTML' | 'Unity' | 'HTML / Unity'

export type RichTextCompletion = {
  tag: string
  format: RichTextFormat
  snippet?: string
  void?: boolean
}

const COMPLETIONS: readonly RichTextCompletion[] = [
  { tag: 'b', format: 'HTML / Unity' },
  { tag: 'i', format: 'HTML / Unity' },
  { tag: 'u', format: 'HTML / Unity' },
  { tag: 's', format: 'HTML / Unity' },
  { tag: 'strong', format: 'HTML' },
  { tag: 'em', format: 'HTML' },
  { tag: 'mark', format: 'HTML' },
  { tag: 'sub', format: 'HTML / Unity' },
  { tag: 'sup', format: 'HTML / Unity' },
  { tag: 'small', format: 'HTML' },
  { tag: 'color', format: 'Unity', snippet: '<color=#ffffff></color>' },
  { tag: 'size', format: 'Unity', snippet: '<size=120%></size>' },
  { tag: 'font-weight', format: 'Unity', snippet: '<font-weight=700></font-weight>' },
  { tag: 'uppercase', format: 'Unity' },
  { tag: 'lowercase', format: 'Unity' },
  { tag: 'smallcaps', format: 'Unity' },
  { tag: 'nobr', format: 'Unity' },
  { tag: 'link', format: 'Unity', snippet: '<link=id></link>' },
  { tag: 'span', format: 'HTML', snippet: '<span style="color: #ffffff"></span>' },
  { tag: 'code', format: 'HTML' },
  { tag: 'br', format: 'HTML', void: true },
  { tag: 'hr', format: 'HTML', void: true },
  { tag: 'space', format: 'Unity', snippet: '<space=0.5em>', void: true },
  { tag: 'sprite', format: 'Unity', snippet: '<sprite=icon>', void: true },
]

type CompletionContext = {
  start: number
  closing: boolean
  query: string
}

export function richTextCompletionContext(value: string, cursor: number): CompletionContext | null {
  const prefix = value.slice(0, cursor)
  const match = prefix.match(/<\/?[A-Za-z-]*$/)
  if (!match || match.index === undefined) return null
  const token = match[0]
  return {
    start: match.index,
    closing: token.startsWith('</'),
    query: token.slice(token.startsWith('</') ? 2 : 1).toLowerCase(),
  }
}

export function richTextSuggestions(value: string, cursor: number): readonly RichTextCompletion[] {
  const context = richTextCompletionContext(value, cursor)
  if (!context) return []
  return COMPLETIONS
    .filter(item => (!context.closing || !item.void) && item.tag.startsWith(context.query))
}

export function applyRichTextCompletion(
  value: string,
  cursor: number,
  completion: RichTextCompletion,
): { value: string, cursor: number } {
  const context = richTextCompletionContext(value, cursor)
  if (!context) return { value, cursor }
  if (context.closing) {
    const inserted = `</${completion.tag}>`
    return {
      value: value.slice(0, context.start) + inserted + value.slice(cursor),
      cursor: context.start + inserted.length,
    }
  }
  const inserted = completion.snippet
    ?? (completion.void ? `<${completion.tag}>` : `<${completion.tag}></${completion.tag}>`)
  const openingEnd = inserted.indexOf('>') + 1
  return {
    value: value.slice(0, context.start) + inserted + value.slice(cursor),
    cursor: context.start + openingEnd,
  }
}

type RichTextInputProps = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'value' | 'onChange'> & {
  value: string
  onValueChange: (value: string) => void
  /** 单元格始终保留同一个输入节点；仅聚焦时按正文高度展开。 */
  cell?: boolean
}

// 尺寸变化按帧合并，所有读、写分阶段执行，避免逐格强制同步布局。
type PreviewMeasurement = { preview: HTMLSpanElement; tableCell: HTMLTableCellElement | null; update: (overflow: boolean) => void }
const previewMeasurements = new Map<Element, Set<PreviewMeasurement>>()
const pendingPreviews = new Set<PreviewMeasurement>()
let previewFrame = 0
let previewObserver: ResizeObserver | undefined
function schedulePreview(item: PreviewMeasurement) {
  pendingPreviews.add(item)
  if (previewFrame) return
  previewFrame = requestAnimationFrame(() => {
    previewFrame = 0
    const items = [...pendingPreviews]
    pendingPreviews.clear()
    const heights = items.map(({ preview, tableCell }) => {
      if (!tableCell || preview.closest('.dc-row')) return null
      const style = getComputedStyle(tableCell)
      return `${Math.max(20, tableCell.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom))}px`
    })
    items.forEach(({ preview }, index) => {
      const height = heights[index]
      if (height !== null && preview.style.height !== height) preview.style.height = height
    })
    const results = items.map(({ preview }) => ({
      height: `${preview.clientHeight}px`,
      overflow: preview.scrollWidth > preview.clientWidth + 1 || preview.scrollHeight > preview.clientHeight + 1,
    }))
    items.forEach(({ preview, update }, index) => {
      preview.parentElement?.style.setProperty('--cell-preview-height', results[index].height)
      update(results[index].overflow)
    })
  })
}
function observePreview(element: Element, item: PreviewMeasurement) {
  previewObserver ??= new ResizeObserver(entries => {
    for (const entry of entries) for (const item of previewMeasurements.get(entry.target) ?? []) schedulePreview(item)
  })
  let items = previewMeasurements.get(element)
  if (!items) { items = new Set(); previewMeasurements.set(element, items); previewObserver.observe(element) }
  items.add(item)
}
function unobservePreview(element: Element, item: PreviewMeasurement) {
  const items = previewMeasurements.get(element)
  items?.delete(item)
  if (!items?.size) { previewMeasurements.delete(element); previewObserver?.unobserve(element) }
  pendingPreviews.delete(item)
}

export function RichTextInput({ value, onValueChange, onKeyDown, onSelect, onFocus, onBlur, cell = false, ...props }: RichTextInputProps) {
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const previewRef = useRef<HTMLSpanElement>(null)
  const entryCaret = useRef<number | null>(null)
  const [overflow, setOverflow] = useState(false)
  const [focused, setFocused] = useState(false)
  const [cursor, setCursor] = useState(0)
  const [active, setActive] = useState(0)
  const [menuPosition, setMenuPosition] = useState({ top: 0, left: 0 })
  const suggestions = useMemo(() => focused && !props.readOnly ? richTextSuggestions(value, cursor) : [], [value, cursor, focused, props.readOnly])

  useLayoutEffect(() => {
    const preview = previewRef.current
    if (!cell || !preview || focused) return
    const tableCell = preview.closest('td')
    const item = { preview, tableCell, update: setOverflow }
    observePreview(preview, item)
    if (tableCell) observePreview(tableCell, item)
    schedulePreview(item)
    return () => {
      unobservePreview(preview, item)
      if (tableCell) unobservePreview(tableCell, item)
    }
  }, [cell, value, focused])

  useLayoutEffect(() => {
    const input = inputRef.current
    if (!cell || !input) return
    // 编辑框仅在聚焦时自适应高度，预览尺寸由共享测量队列维护。
    if (!focused) {
      input.style.height = ''
      return
    }
    const resize = () => {
      input.style.height = '0px'
      input.style.height = `${input.scrollHeight}px`
    }
    resize()
    // 只监听宽度，避免自身高度变化触发 ResizeObserver 循环。
    let width = input.getBoundingClientRect().width
    const observer = new ResizeObserver(entries => {
      const nextWidth = entries[0]?.contentRect.width ?? width
      if (nextWidth === width) return
      width = nextWidth
      resize()
    })
    observer.observe(input)
    return () => observer.disconnect()
  }, [cell, focused, value])

  useLayoutEffect(() => {
    if (suggestions.length === 0) return
    const update = () => {
      const rect = inputRef.current?.getBoundingClientRect()
      if (!rect) return
      const menuHeight = Math.min(suggestions.length * 28 + 8, 232)
      const top = window.innerHeight - rect.bottom >= menuHeight + 3
        ? rect.bottom + 3
        : Math.max(8, rect.top - menuHeight - 3)
      const left = Math.max(8, Math.min(rect.left, window.innerWidth - 250))
      setMenuPosition({ top, left })
    }
    update()
    window.addEventListener('resize', update)
    document.addEventListener('scroll', update, true)
    return () => {
      window.removeEventListener('resize', update)
      document.removeEventListener('scroll', update, true)
    }
  }, [suggestions.length])

  function updateCursor(element: HTMLTextAreaElement) {
    setCursor(element.selectionStart)
    setActive(0)
  }

  function choose(completion: RichTextCompletion) {
    const result = applyRichTextCompletion(value, cursor, completion)
    const element = inputRef.current
    if (!element) return
    onValueChange(result.value)
    requestAnimationFrame(() => {
      element.focus()
      element.setSelectionRange(result.cursor, result.cursor)
      setCursor(result.cursor)
    })
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    // 中文输入法确认候选词不能触发补全或提交。
    if (event.nativeEvent.isComposing || event.keyCode === 229) return
    if (suggestions.length > 0) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const delta = event.key === 'ArrowDown' ? 1 : -1
        setActive(index => (index + delta + suggestions.length) % suggestions.length)
        return
      }
      if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
        event.preventDefault()
        choose(suggestions[active] ?? suggestions[0])
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setCursor(-1)
        return
      }
    }
    onKeyDown?.(event)
  }

  return (
    <span className={`rich-text-input${cell ? ' cell-string-input' : ''}${focused ? ' focused' : ''}`}>
      {cell && <span ref={previewRef} className="cell-string-preview" aria-hidden>{value || '\u00a0'}</span>}
      {cell && !focused && overflow && <span className="cell-string-overflow" aria-hidden>…</span>}
      <textarea
        {...props}
        ref={inputRef}
        value={value}
        onPointerDown={event => {
          entryCaret.current = null
          if (!cell || focused || event.button !== 0 || event.defaultPrevented) return
          const preview = previewRef.current
          const input = event.currentTarget
          if (!preview) return
          // 使用浏览器字形命中结果定位光标；进入编辑的双击不再选中整个词。
          input.style.pointerEvents = 'none'
          preview.style.pointerEvents = 'auto'
          const range = document.caretRangeFromPoint(event.clientX, event.clientY)
          input.style.pointerEvents = ''
          preview.style.pointerEvents = ''
          const offset = range && preview.contains(range.startContainer) ? range.startOffset : value.length
          entryCaret.current = offset
          event.preventDefault()
          input.focus({ preventScroll: true })
          input.setSelectionRange(offset, offset)
        }}
        onDoubleClick={event => {
          if (entryCaret.current === null) return
          event.preventDefault()
          event.currentTarget.setSelectionRange(entryCaret.current, entryCaret.current)
        }}
        onChange={event => {
          onValueChange(event.currentTarget.value)
          updateCursor(event.currentTarget)
        }}
        onFocus={event => {
          setFocused(true)
          updateCursor(event.currentTarget)
          onFocus?.(event)
        }}
        onBlur={event => {
          setFocused(false)
          onBlur?.(event)
        }}
        onSelect={event => {
          updateCursor(event.currentTarget)
          onSelect?.(event)
        }}
        onKeyDown={handleKeyDown}
      />
      {suggestions.length > 0 && createPortal(
        <span
          className="rich-text-suggestions"
          role="listbox"
          style={{ top: menuPosition.top, left: menuPosition.left }}
        >
          {suggestions.map((item, index) => (
            <button
              className={index === active ? 'active' : undefined}
              type="button"
              role="option"
              aria-selected={index === active}
              key={item.tag}
              onMouseDown={event => event.preventDefault()}
              onClick={() => choose(item)}
            >
              <code>{`<${item.tag}>`}</code>
              <span>{item.format}</span>
            </button>
          ))}
        </span>,
        document.body,
      )}
    </span>
  )
}
