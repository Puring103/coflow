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
}

export function RichTextInput({ value, onValueChange, onKeyDown, onSelect, onFocus, ...props }: RichTextInputProps) {
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const [cursor, setCursor] = useState(0)
  const [active, setActive] = useState(0)
  const [menuPosition, setMenuPosition] = useState({ top: 0, left: 0 })
  const suggestions = useMemo(() => richTextSuggestions(value, cursor), [value, cursor])

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
    if (suggestions.length > 0) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const delta = event.key === 'ArrowDown' ? 1 : -1
        setActive(index => (index + delta + suggestions.length) % suggestions.length)
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
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
    <span className="rich-text-input">
      <textarea
        {...props}
        ref={inputRef}
        value={value}
        onChange={event => {
          onValueChange(event.currentTarget.value)
          updateCursor(event.currentTarget)
        }}
        onFocus={event => {
          updateCursor(event.currentTarget)
          onFocus?.(event)
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
