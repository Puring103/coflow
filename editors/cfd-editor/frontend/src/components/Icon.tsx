interface IconProps {
  name:
    | 'folder' | 'file' | 'file-cfd'
    | 'arrow-left' | 'arrow-right'
    | 'plus' | 'close' | 'search' | 'help'
    | 'chevron-right' | 'chevron-down' | 'chevron-up'
    | 'error' | 'warning' | 'info' | 'check'
    | 'jump' | 'open' | 'dot' | 'edit'
    | 'table' | 'record' | 'graph' | 'filter' | 'sun' | 'moon' | 'lock'
    | 'download' | 'refresh' | 'build'
    | 'sparkles' | 'settings'
  size?: number
  className?: string
}

const PATHS: Record<IconProps['name'], string> = {
  'folder':        'M3 5a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414A1 1 0 0011.414 5H17a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V5z',
  'file':          'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5',
  'file-cfd':      'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5M9 14h6M9 17h4',
  'arrow-left':    'M14 6l-6 6 6 6',
  'arrow-right':   'M10 6l6 6-6 6',
  'plus':          'M12 5v14M5 12h14',
  'close':         'M6 6l12 12M6 18L18 6',
  'search':        'M11 4a7 7 0 105.196 11.804L21 20.5M11 4a7 7 0 016.196 10.196',
  'help':          'M9.5 9a2.5 2.5 0 015 0c0 1.5-2.5 2-2.5 4M12 18h.01',
  'chevron-right': 'M9 6l6 6-6 6',
  'chevron-down':  'M6 9l6 6 6-6',
  'chevron-up':    'M6 15l6-6 6 6',
  'error':         'M12 9v4M12 17h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'warning':       'M12 9v4M12 17h.01M10.3 3.86l-8.4 14.5A2 2 0 003.6 21.4h16.8a2 2 0 001.7-3.04l-8.4-14.5a2 2 0 00-3.4 0z',
  'info':          'M12 16v-4M12 8h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'check':         'M5 12l5 5L20 7',
  'jump':          'M7 17L17 7M17 7H8M17 7v9',
  'open':          'M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7z',
  'dot':           'M12 12m-3 0a3 3 0 106 0 3 3 0 10-6 0',
  'edit':          'M4 20h4L18.5 9.5a2.1 2.1 0 00-3-3L5 17v3zM13.5 7.5l3 3',
  'table':         'M3 4h18v16H3zM3 10h18M9 4v16',
  'record':        'M4 6h16M4 12h16M4 18h16',
  'graph':         'M5 19V8m0 0a2 2 0 100-4 2 2 0 000 4zm14 11V12m0 0a2 2 0 100-4 2 2 0 000 4zM7 19l5-7 5 4',
  'filter':        'M4 5h16l-6 8v6l-4-2v-4z',
  'sun':           'M12 4V2M12 22v-2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41M16 12a4 4 0 11-8 0 4 4 0 018 0z',
  'moon':          'M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z',
  'lock':          'M5 11h14v10H5zM8 11V7a4 4 0 018 0v4',
  'download':      'M12 3v12M7 10l5 5 5-5M5 21h14',
  'refresh':       'M20 6v5h-5M4 18v-5h5M18.5 9A7 7 0 006.7 6.7L4 11M5.5 15A7 7 0 0017.3 17.3L20 13',
  'build':         'M14.7 6.3a4 4 0 01-5 5L4 17l3 3 5.7-5.7a4 4 0 005-5l-2.4 2.4-3-3L14.7 6.3z',
  'sparkles':      'M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5L12 3zM19 14l.75 2.25L22 17l-2.25.75L19 20l-.75-2.25L16 17l2.25-.75L19 14z',
  'settings':      'M12 15a3 3 0 100-6 3 3 0 000 6zM19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.6 15a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z',
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
