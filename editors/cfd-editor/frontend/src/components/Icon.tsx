interface IconProps {
  name:
    | 'folder' | 'file' | 'file-cfd'
    | 'arrow-left' | 'arrow-right' | 'arrow-up' | 'arrow-down'
    | 'plus' | 'close' | 'search' | 'help'
    | 'chevron-right' | 'chevron-down' | 'chevron-up'
    | 'error' | 'warning' | 'info' | 'check'
    | 'jump' | 'open' | 'expand' | 'reference' | 'dot' | 'edit'
    | 'table' | 'record' | 'graph' | 'filter' | 'sun' | 'moon' | 'lock'
    | 'download' | 'refresh' | 'build' | 'copy' | 'paste'
    | 'sparkles' | 'gear' | 'settings' | 'data' | 'localization' | 'dimension' | 'grip'
    | 'extensions' | 'code' | 'git-branch' | 'save' | 'trash' | 'undo' | 'redo'
    | 'key' | 'panel-bottom' | 'panel-bottom-closed' | 'panel-left-closed' | 'panel-right-closed' | 'panel-left' | 'panel-right' | 'layout' | 'overview' | 'center' | 'card-detail' | 'card-compact'
  size?: number
  className?: string
}

const PATHS: Record<IconProps['name'], string> = {
  // 引用统一使用同一条矢量 & 轮廓，避免摘要和输入框继承不同字体。
  'reference': 'M17.5 20 9 11.5C6.5 9 6 7.5 6 6a4 4 0 0 1 8 0c0 2-1 3-4 5l-3 2a4.5 4.5 0 0 0 5 7c3-2 5-5 6-8M16 12h4',
  'undo': 'M9 4L4 9l5 5M4 9h10a6 6 0 010 12',
  'redo': 'M15 4l5 5-5 5M20 9H10a6 6 0 000 12',
  // Lucide KeyRound：圆头钥匙。
  'key': 'M2.586 17.414A2 2 0 002 18.828V21a1 1 0 001 1h3a1 1 0 001-1v-1a1 1 0 011-1h1a1 1 0 001-1v-1a1 1 0 011-1h.172a2 2 0 001.414-.586l.814-.814a6.5 6.5 0 10-4-4zM16.5 7.5h.01',
  'panel-left-closed': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM9 3v18',
  'panel-right-closed': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM15 3v18',
  'panel-bottom': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 15h18',
  'panel-bottom-closed': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 15h18',
  'panel-right': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM15 3v18',
  'panel-left': 'M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM9 3v18',
  'gear': 'M9.5 3h5l.6 2.5 2 1.2 2.5-.7 2.5 4.3-1.9 1.7v2l1.9 1.7-2.5 4.3-2.5-.7-2 1.2-.6 2.5h-5l-.6-2.5-2-1.2-2.5.7L1.9 15.7 3.8 14v-2L1.9 10.3 4.4 6l2.5.7 2-1.2L9.5 3zM15 13a3 3 0 11-6 0 3 3 0 016 0z',
  'folder': 'M3 7V5a2 2 0 012-2h5l2 3h7a2 2 0 012 2v11a2 2 0 01-2 2H5a2 2 0 01-2-2V7z',
  'file':          'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5',
  'file-cfd':      'M6 3h7l5 5v11a2 2 0 01-2 2H6a2 2 0 01-2-2V5a2 2 0 012-2zm7 0v5h5M9 14h6M9 17h4',
  'arrow-left':    'M14 6l-6 6 6 6',
  'arrow-right':   'M10 6l6 6-6 6',
  'arrow-up':      'M12 19V5M5 12l7-7 7 7',
  'arrow-down':    'M12 5v14M5 12l7 7 7-7',
  'plus':          'M12 5v14M5 12h14',
  'close':         'M6 6l12 12M6 18L18 6',
  'search': 'M10.5 3a7.5 7.5 0 110 15 7.5 7.5 0 010-15zM16 16l5 5',
  'help': 'M12 3a9 9 0 110 18 9 9 0 010-18zM9.5 9a2.5 2.5 0 015 0c0 2-2.5 2-2.5 4M12 16.5v.1',
  'chevron-right': 'M9 6l6 6-6 6',
  'chevron-down':  'M6 9l6 6 6-6',
  'chevron-up':    'M6 15l6-6 6 6',
  'error':         'M12 9v4M12 17h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'warning':       'M12 9v4M12 17h.01M10.3 3.86l-8.4 14.5A2 2 0 003.6 21.4h16.8a2 2 0 001.7-3.04l-8.4-14.5a2 2 0 00-3.4 0z',
  'info':          'M12 16v-4M12 8h.01M3 12a9 9 0 1018 0 9 9 0 00-18 0z',
  'check':         'M5 12l5 5L20 7',
  'jump':          'M7 17L17 7M17 7H8M17 7v9',
  // 四角向外展开，区别于文件夹的打开操作。
  'expand': 'M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5M3 3l6 6M21 3l-6 6M3 21l6-6M21 21l-6-6',
  'open':          'M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7z',
  'dot':           'M12 12m-3 0a3 3 0 106 0 3 3 0 10-6 0',
  'edit':          'M4 20h4L18.5 9.5a2.1 2.1 0 00-3-3L5 17v3zM13.5 7.5l3 3',
  'table':         'M3 4h18v16H3zM3 10h18M9 4v16',
  'record':        'M4 6h16M4 12h16M4 18h16',
  'graph':         'M5 3h4a2 2 0 012 2v4a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM15 13h4a2 2 0 012 2v4a2 2 0 01-2 2h-4a2 2 0 01-2-2v-4a2 2 0 012-2zM7 11v4a2 2 0 002 2h4',
  'filter':        'M4 5h16l-6 8v6l-4-2v-4z',
  'sun':           'M12 4V2M12 22v-2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41M16 12a4 4 0 11-8 0 4 4 0 018 0z',
  'moon':          'M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z',
  'lock':          'M5 11h14v10H5zM8 11V7a4 4 0 018 0v4',
  'download':      'M12 3v12M7 10l5 5 5-5M5 21h14',
  'refresh':       'M20 6v5h-5M4 18v-5h5M18.5 9A7 7 0 006.7 6.7L4 11M5.5 15A7 7 0 0017.3 17.3L20 13',
  'build': 'M4 7l8-4 8 4v10l-8 4-8-4V7zM4 7l8 4 8-4M12 11v10M8 5l8 4',
  'copy':          'M8 8h11v11H8zM5 16H4V5h11v1',
  'paste':         'M9 5h6M9 3h6v4H9zM7 5H5v16h14V5h-2',
  'sparkles':      'M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5L12 3zM19 14l.75 2.25L22 17l-2.25.75L19 20l-.75-2.25L16 17l2.25-.75L19 14z',
  'settings':      'M4 6h10M18 6h2M14 4v4M4 12h3M11 12h9M7 10v4M4 18h10M18 18h2M14 16v4',
  'data':          'M4 6c0 1.66 3.58 3 8 3s8-1.34 8-3-3.58-3-8-3-8 1.34-8 3zM4 6v6c0 1.66 3.58 3 8 3s8-1.34 8-3V6M4 12v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6',
  'localization':  'M3 5h12M9 3v2M5 9c1.5 3 4 5 7 6M12 9c-1.5 3-4 5-7 6M14 21l4-10 4 10M15.5 17h5',
  'dimension':     'M4 19V5M4 19h14M4 19l6-6M10 13h7M10 13V6M10 13l7-7',
  'grip':          'M9 5h.01M9 12h.01M9 19h.01M15 5h.01M15 12h.01M15 19h.01',
  'extensions': 'M4 4h6v2a2 2 0 004 0V4h6v6h-2a2 2 0 000 4h2v6h-6v-2a2 2 0 00-4 0v2H4v-6h2a2 2 0 000-4H4z',
  'code':          'M8 9l-4 3 4 3M16 9l4 3-4 3M14 5l-4 14',
  'git-branch': 'M6 8v8M6 8a3 3 0 100-6 3 3 0 000 6zM6 22a3 3 0 100-6 3 3 0 000 6zM18 8a3 3 0 100-6 3 3 0 000 6zM18 8v3a8 8 0 01-8 8H9',
  'save':          'M5 3h12l2 2v16H5zM8 3v6h8V3M8 21v-7h8v7',
  'trash':         'M3 6h18M8 6V4h8v2M19 6l-1 15H6L5 6M10 11v6M14 11v6',
  // 自动布局：树状层级（父节点连两个子节点）。
  'layout': 'M3 3h6v6H3zM15 3h6v6h-6zM3 15h6v6H3zM15 15h6v6h-6zM9 6h6M6 9v6M18 9v6',
  'card-detail': 'M4 3h16v18H4zM4 9h16M8 13h8M8 17h6',
  'card-compact': 'M4 7h16v10H4zM8 12h8',
  // 缩略/完整模式：眼睛（概览）。
  'overview':      'M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6zM12 15a3 3 0 100-6 3 3 0 000 6z',
  // 回正：四角箭头指向中心（缩放到整张图）。
  'center':        'M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5M8 8h8v8H8z',
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
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={{ flexShrink: 0 }}
    >
      {/* 收起状态填充对应面板区域，与展开状态保持同一轮廓。 */}
      {name === 'panel-left-closed' && <path d="M5 3h4v18H5a2 2 0 01-2-2V5a2 2 0 012-2z" fill="currentColor" stroke="none" />}
      {name === 'panel-right-closed' && <path d="M15 3h4a2 2 0 012 2v14a2 2 0 01-2 2h-4z" fill="currentColor" stroke="none" />}
      {name === 'panel-bottom-closed' && <path d="M3 15h18v4a2 2 0 01-2 2H5a2 2 0 01-2-2z" fill="currentColor" stroke="none" />}
      <path d={d} />
    </svg>
  )
}
