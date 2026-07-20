import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from 'react'
import { createPortal } from 'react-dom'

export interface SearchableOption {
  value: string
  label?: string
}

interface Props {
  value: string
  options: readonly SearchableOption[]
  onCommit: (value: string) => void
  onExit?: () => void
  className?: string
  style?: CSSProperties
  title?: string
  placeholder?: string
  ariaLabel?: string
  autoFocus?: boolean
}

interface MenuPosition {
  left: number
  top: number
  width: number
  maxHeight: number
  opensUpward: boolean
}

export function filterSearchableOptions(
  options: readonly SearchableOption[],
  query: string,
): SearchableOption[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean)
  if (terms.length === 0) return [...options]

  return options.filter(option => {
    const searchableText = `${option.label ?? ''} ${option.value}`.toLocaleLowerCase()
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
  onExit,
  className,
  style,
  title,
  placeholder,
  ariaLabel,
  autoFocus,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null)
  const exitNotifiedRef = useRef(false)
  const listboxId = useId()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const [menuPosition, setMenuPosition] = useState<MenuPosition | null>(null)

  const filteredOptions = useMemo(
    () => filterSearchableOptions(options, query),
    [options, query],
  )
  const selectedOption = options.find(option => option.value === value)
  const displayValue = selectedOption?.label ?? selectedOption?.value ?? value
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
    const selectedIndex = options.findIndex(option => option.value === value)
    setActiveIndex(Math.max(0, selectedIndex))
    setOpen(true)
  }, [options, value])

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

  const chooseOption = useCallback((option: SearchableOption) => {
    onCommit(option.value)
    closeMenu(true)
    inputRef.current?.blur()
  }, [closeMenu, onCommit])

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
    document.getElementById(`${listboxId}-option-${activeIndex}`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex, activeOption, listboxId, open])

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

  const menu = open && menuPosition && createPortal(
    <div
      id={listboxId}
      className="searchable-select-menu"
      role="listbox"
      aria-label={ariaLabel ?? title ?? placeholder ?? '选项'}
      style={{
        left: menuPosition.left,
        top: menuPosition.top,
        width: menuPosition.width,
        maxHeight: menuPosition.maxHeight,
        transform: menuPosition.opensUpward ? 'translateY(-100%)' : undefined,
      }}
      onMouseDown={event => event.preventDefault()}
    >
      {filteredOptions.length === 0 ? (
        <div className="searchable-select-empty">无匹配项</div>
      ) : filteredOptions.map((option, index) => {
        const selected = option.value === value
        const active = index === activeIndex
        return (
          <button
            id={`${listboxId}-option-${index}`}
            key={option.value}
            type="button"
            className={`searchable-select-option${active ? ' active' : ''}${selected ? ' selected' : ''}`}
            role="option"
            aria-selected={selected}
            title={option.label ?? option.value}
            onMouseDown={event => event.preventDefault()}
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => chooseOption(option)}
          >
            <span>{option.label ?? option.value}</span>
            {selected && <span className="searchable-select-check" aria-hidden="true">✓</span>}
          </button>
        )
      })}
    </div>,
    document.body,
  )

  return (
    <>
      <input
        ref={inputRef}
        className={`${className ?? ''} searchable-select${open ? ' searchable-select-open' : ''}`.trim()}
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
        onClick={() => {
          if (!open) openMenu()
        }}
        onChange={event => {
          const nextQuery = event.target.value
          setQuery(nextQuery)
          if (nextQuery.trim() === '') {
            const selectedIndex = options.findIndex(option => option.value === value)
            setActiveIndex(Math.max(0, selectedIndex))
          } else {
            setActiveIndex(0)
          }
        }}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          if (open) closeMenu(true)
        }}
      />
      {menu}
    </>
  )
}
