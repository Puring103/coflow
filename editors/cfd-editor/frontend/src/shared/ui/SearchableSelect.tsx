import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type MouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { Icon } from './Icon'

export interface SearchableOption {
  value: string
  label?: string
  selectedLabel?: string
  description?: string
}

interface Props {
  value: string
  options: readonly SearchableOption[]
  onCommit: (value: string) => void
  selectedValues?: readonly string[]
  onToggle?: (value: string) => void
  onExit?: () => void
  className?: string
  style?: CSSProperties
  title?: string
  placeholder?: string
  ariaLabel?: string
  autoFocus?: boolean
  onModifiedClick?: () => void
  onPreview?: () => void
  reference?: boolean
}

interface MenuPosition {
  left: number
  top: number
  width: number
  maxHeight: number
  opensUpward: boolean
}

const optionIndexes = new WeakMap<readonly SearchableOption[], Map<string, number>>()
const optionSearchTexts = new WeakMap<readonly SearchableOption[], string[]>()
function optionIndex(options: readonly SearchableOption[]): Map<string, number> {
  let index = optionIndexes.get(options)
  if (!index) {
    index = new Map()
    options.forEach((option, position) => { if (!index!.has(option.value)) index!.set(option.value, position) })
    optionIndexes.set(options, index)
  }
  return index
}

export function filterSearchableOptions(
  options: readonly SearchableOption[],
  query: string,
): readonly SearchableOption[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean)
  if (terms.length === 0) return options

  let texts = optionSearchTexts.get(options)
  if (!texts) {
    texts = options.map(option => `${option.selectedLabel ?? ''} ${option.label ?? ''} ${option.value}`.toLocaleLowerCase())
    optionSearchTexts.set(options, texts)
  }
  return options.filter((_option, index) => {
    // 缩略显示名与完整标签、记录 ID 都参与搜索。
    const searchableText = texts![index]
    return terms.every(term => searchableText.includes(term))
  })
}

export function moveSearchableOptionIndex(
  current: number,
  optionCount: number,
  direction: 1 | -1,
): number {
  if (optionCount <= 0) return 0
  return (current + direction + optionCount) % optionCount
}

export function SearchableSelect({
  value,
  options,
  onCommit,
  selectedValues,
  onToggle,
  onExit,
  className,
  style,
  title,
  placeholder,
  ariaLabel,
  autoFocus,
  onModifiedClick,
  onPreview,
  reference = false,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null)
  const exitNotifiedRef = useRef(false)
  const menuPointerDownRef = useRef(false)
  const listboxId = useId()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const [menuPosition, setMenuPosition] = useState<MenuPosition | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const [menuScrollTop, setMenuScrollTop] = useState(0)

  const filteredOptions = useMemo(
    () => open ? filterSearchableOptions(options, query) : options,
    [options, query, open],
  )
  const selectedIndex = optionIndex(options).get(value) ?? -1
  const selectedOption = options[selectedIndex]
  const displayValue = selectedOption?.selectedLabel ?? selectedOption?.label ?? selectedOption?.value ?? value
  const activeOption = filteredOptions[activeIndex]
  const activeLabel = activeOption?.label ?? activeOption?.value

  const updateMenuPosition = useCallback(() => {
    const input = inputRef.current
    if (!input) return

    const rect = input.getBoundingClientRect()
    const viewportPadding = 8
    const gap = 4
    const availableBelow = window.innerHeight - rect.bottom - gap - viewportPadding
    const availableAbove = rect.top - gap - viewportPadding
    const opensUpward = availableBelow < 120 && availableAbove > availableBelow
    const availableHeight = opensUpward ? availableAbove : availableBelow
    const width = Math.min(Math.max(rect.width, 220), window.innerWidth - viewportPadding * 2)
    const left = Math.max(
      viewportPadding,
      Math.min(rect.left, window.innerWidth - width - viewportPadding),
    )

    setMenuPosition({
      left,
      top: opensUpward ? rect.top - gap : rect.bottom + gap,
      width,
      maxHeight: Math.max(72, Math.min(240, availableHeight)),
      opensUpward,
    })
  }, [])

  const openMenu = useCallback(() => {
    exitNotifiedRef.current = false
    setQuery('')
    setActiveIndex(Math.max(0, selectedIndex))
    setMenuScrollTop(Math.max(0, selectedIndex - 3) * 28)
    setOpen(true)
  }, [selectedIndex])

  const notifyExit = useCallback(() => {
    if (exitNotifiedRef.current) return
    exitNotifiedRef.current = true
    requestAnimationFrame(() => onExit?.())
  }, [onExit])

  const closeMenu = useCallback((notify = false) => {
    setOpen(false)
    setQuery('')
    if (notify) notifyExit()
  }, [notifyExit])

  const selectedSet = useMemo(() => new Set(selectedValues), [selectedValues])

  useEffect(() => {
    if (!open) return
    // 捕获外部交互，避免面板或单元格阻止冒泡、保留焦点时遗留下拉。
    const dismiss = (event: Event) => {
      const target = event.target
      if (!(target instanceof Node)) return
      if (inputRef.current?.parentElement?.contains(target)
        || document.getElementById(listboxId)?.contains(target)) return
      closeMenu(true)
    }
    const blur = () => closeMenu(true)
    document.addEventListener('pointerdown', dismiss, true)
    document.addEventListener('focusin', dismiss, true)
    window.addEventListener('blur', blur)
    return () => {
      document.removeEventListener('pointerdown', dismiss, true)
      document.removeEventListener('focusin', dismiss, true)
      window.removeEventListener('blur', blur)
    }
  }, [open, closeMenu, listboxId])

  const chooseOption = useCallback((option: SearchableOption) => {
    if (onToggle) {
      // A portaled option can still blur the trigger in some browser event
      // orders. Multi-select interactions must leave the menu available for
      // the next toggle.
      exitNotifiedRef.current = false
      setOpen(true)
      onToggle(option.value)
      setQuery('')
      inputRef.current?.focus()
      return
    }
    onCommit(option.value)
    closeMenu(true)
    inputRef.current?.blur()
  }, [closeMenu, onCommit, onToggle])

  useLayoutEffect(() => {
    if (open) updateMenuPosition()
    else setMenuPosition(null)
  }, [open, filteredOptions.length, updateMenuPosition])

  useEffect(() => {
    if (!open) return
    const reposition = () => updateMenuPosition()
    window.addEventListener('resize', reposition)
    window.addEventListener('scroll', reposition, true)
    return () => {
      window.removeEventListener('resize', reposition)
      window.removeEventListener('scroll', reposition, true)
    }
  }, [open, updateMenuPosition])

  useEffect(() => {
    if (activeIndex >= filteredOptions.length) {
      setActiveIndex(Math.max(0, filteredOptions.length - 1))
    }
  }, [activeIndex, filteredOptions.length])

  function moveActiveOption(direction: 1 | -1) {
    if (!open) {
      openMenu()
      return
    }
    if (filteredOptions.length === 0) return
    setActiveIndex(current => moveSearchableOptionIndex(
      current,
      filteredOptions.length,
      direction,
    ))
  }

  useEffect(() => {
    if (!open || !activeOption) return
    if (filteredOptions.length > 100) {
      const menu = menuRef.current
      if (!menu) return
      const top = activeIndex * 28
      if (top < menu.scrollTop) menu.scrollTop = top
      else if (top + 28 > menu.scrollTop + menu.clientHeight - 8) menu.scrollTop = top + 36 - menu.clientHeight
      setMenuScrollTop(menu.scrollTop)
      return
    }
    document.getElementById(`${listboxId}-option-${activeIndex}`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex, activeOption, listboxId, open, menuPosition, filteredOptions.length])

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      event.stopPropagation()
      moveActiveOption(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      event.stopPropagation()
      moveActiveOption(-1)
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      if (!open) openMenu()
      else if (filteredOptions[activeIndex]) chooseOption(filteredOptions[activeIndex])
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closeMenu(true)
      event.currentTarget.blur()
    }
  }

  // 长列表只挂载视口与活动项，键盘跳转先滚动再发布窗口，ARIA 身份保持稳定。
  const virtualMenu = filteredOptions.length > 100
  const firstOption = virtualMenu ? Math.min(filteredOptions.length - 1, Math.max(0, Math.floor(menuScrollTop / 28) - 4)) : 0
  const lastOption = virtualMenu ? Math.min(filteredOptions.length, firstOption + 18) : filteredOptions.length
  const menu = open && menuPosition && createPortal(
    <div
      ref={menuRef}
      onScroll={event => setMenuScrollTop(event.currentTarget.scrollTop)}
      id={listboxId}
      className="searchable-select-menu"
      role="listbox"
      aria-multiselectable={onToggle ? true : undefined}
      aria-label={ariaLabel ?? title ?? placeholder ?? '选项'}
      style={{
        left: menuPosition.left,
        top: menuPosition.top,
        width: menuPosition.width,
        maxHeight: menuPosition.maxHeight,
        transform: menuPosition.opensUpward ? 'translateY(-100%)' : undefined,
      }}
      onMouseDown={event => {
        menuPointerDownRef.current = true
        event.preventDefault()
        event.stopPropagation()
      }}
      onClick={event => {
        menuPointerDownRef.current = false
        event.stopPropagation()
      }}
    >
      {filteredOptions.length === 0 ? (
        <div className="searchable-select-empty">无匹配项</div>
      ) : <>
      {virtualMenu && <div aria-hidden style={{ height: firstOption * 28, flexShrink: 0 }} />}
      {filteredOptions.slice(firstOption, lastOption).map((option, offset) => {
        const index = firstOption + offset
        const selected = onToggle ? selectedSet.has(option.value) : option.value === value
        const active = index === activeIndex
        return (
          <button
            id={`${listboxId}-option-${index}`}
            key={option.value}
            type="button"
            className={`searchable-select-option${active ? ' active' : ''}${selected ? ' selected' : ''}`}
            role="option"
            aria-posinset={index + 1}
            aria-setsize={filteredOptions.length}
            style={virtualMenu ? { height: 28 } : undefined}
            aria-selected={selected}
            title={option.description ?? option.label ?? option.value}
            onMouseDown={event => {
              menuPointerDownRef.current = true
              event.preventDefault()
            }}
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => chooseOption(option)}
          >
            <span>{option.label ?? option.value}</span>
            {onToggle && (
              <input
                className="searchable-select-checkbox"
                type="checkbox"
                checked={selected}
                readOnly
                tabIndex={-1}
                aria-hidden="true"
              />
            )}
            {!onToggle && selected && <span className="searchable-select-check" aria-hidden="true">✓</span>}
          </button>
        )
      })}
      {virtualMenu && <div aria-hidden style={{ height: (filteredOptions.length - lastOption) * 28, flexShrink: 0 }} />}
      </>}
    </div>,
    document.body,
  )

  return (
    <span className={reference ? "searchable-reference-wrap" : undefined}>
      {reference && <button type="button" className="reference-open" aria-label="打开引用记录" title="打开引用记录" disabled={!onPreview} onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); closeMenu(); inputRef.current?.blur(); onPreview?.() }}><Icon name="reference" size={12} /></button>}
      <input
        ref={inputRef}
        className={`${className ?? ''} searchable-select${reference ? ' searchable-select-reference' : ''}${open ? ' searchable-select-open' : ''}`.trim()}
        style={style}
        title={title}
        role="combobox"
        aria-label={ariaLabel ?? title ?? placeholder ?? '可搜索选项'}
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={listboxId}
        aria-activedescendant={open && filteredOptions[activeIndex]
          ? `${listboxId}-option-${activeIndex}`
          : undefined}
        value={open ? query : displayValue}
        placeholder={open ? activeLabel ?? '搜索...' : placeholder}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        onFocus={() => {
          if (!open) openMenu()
        }}
        onClick={(event: MouseEvent<HTMLInputElement>) => {
          if ((event.ctrlKey || event.metaKey) && onModifiedClick) {
            event.preventDefault()
            event.stopPropagation()
            closeMenu()
            event.currentTarget.blur()
            onModifiedClick()
            return
          }
          if (!open) openMenu()
        }}
        onChange={event => {
          const nextQuery = event.target.value
          setQuery(nextQuery)
          if (menuRef.current) menuRef.current.scrollTop = 0
          setMenuScrollTop(0)
          if (nextQuery.trim() === '') {
            setActiveIndex(Math.max(0, selectedIndex))
          } else {
            setActiveIndex(0)
          }
        }}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          if (menuPointerDownRef.current && onToggle) return
          if (open) closeMenu(true)
        }}
      />

      {menu}
    </span>
  )
}
