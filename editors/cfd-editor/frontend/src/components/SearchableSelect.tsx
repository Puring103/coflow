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
  style?: React.CSSProperties
  title?: string
  placeholder?: string
  ariaLabel?: string
  autoFocus?: boolean
}

/** Native selects preserve the platform picker and provide built-in
 * incremental search when the user types while the picker is open. */
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
  const exit = (element: HTMLSelectElement) => {
    element.blur()
    requestAnimationFrame(() => onExit?.())
  }

  return (
    <select
      className={`${className ?? ''} searchable-select`.trim()}
      style={style}
      title={title}
      aria-label={ariaLabel}
      value={value}
      autoFocus={autoFocus}
      onChange={e => {
        onCommit(e.target.value)
        exit(e.currentTarget)
      }}
      onKeyDown={e => {
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          exit(e.currentTarget)
        }
      }}
    >
      {placeholder && value === '' && <option value="" disabled>{placeholder}</option>}
      {options.map(option => (
        <option key={option.value} value={option.value}>{option.label ?? option.value}</option>
      ))}
    </select>
  )
}
