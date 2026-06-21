interface IconProps {
  name:
    | 'folder' | 'file' | 'file-ghost'
    | 'arrow-left' | 'arrow-right'
    | 'plus' | 'close' | 'search' | 'help'
    | 'chevron-right' | 'chevron-down'
    | 'error' | 'warning' | 'info' | 'check'
    | 'jump' | 'open' | 'dot'
    | 'table' | 'record' | 'graph'
  size?: number
  className?: string
}

const PATHS: Record<IconProps['name'], string> = {
  'folder':        'M3 5a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414A1 1 0 0011.414 5H17a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V5z',
  'file':          'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5',
  'file-ghost':    'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5',
  'arrow-left':    'M14 6l-6 6 6 6',
  'arrow-right':   'M10 6l6 6-6 6',
  'plus':          'M12 5v14M5 12h14',
  'close':         'M6 6l12 12M6 18L18 6',
  'search':        'M11 4a7 7 0 105.196 11.804L21 20.5M11 4a7 7 0 016.196 10.196',
  'help':          'M9.5 9a2.5 2.5 0 015 0c0 1.5-2.5 2-2.5 4M12 18h.01',
  'chevron-right': 'M9 6l6 6-6 6',
  'chevron-down':  'M6 9l6 6 6-6',
  'error':         'M12 9v4M12 17h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'warning':       'M12 9v4M12 17h.01M10.3 3.86l-8.4 14.5A2 2 0 003.6 21.4h16.8a2 2 0 001.7-3.04l-8.4-14.5a2 2 0 00-3.4 0z',
  'info':          'M12 16v-4M12 8h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'check':         'M5 12l5 5L20 7',
  'jump':          'M7 17L17 7M17 7H8M17 7v9',
  'open':          'M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7z',
  'dot':           'M12 12m-3 0a3 3 0 106 0 3 3 0 10-6 0',
  'table':         'M3 4h18v16H3zM3 10h18M9 4v16',
  'record':        'M4 6h16M4 12h16M4 18h16',
  'graph':         'M5 19V8m0 0a2 2 0 100-4 2 2 0 000 4zm14 11V12m0 0a2 2 0 100-4 2 2 0 000 4zM7 19l5-7 5 4',
}

export function Icon({ name, size = 14, className }: IconProps) {
  const d = PATHS[name]
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={{ flexShrink: 0 }}
    >
      <path d={d} />
    </svg>
  )
}
