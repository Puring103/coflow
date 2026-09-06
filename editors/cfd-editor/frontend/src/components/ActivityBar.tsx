import { useEffect, useRef, useState } from 'react'
import type { PluginRegistrySnapshot } from '../plugins'
import { Icon } from './Icon'

export type ActivePane = 'files' | 'changes' | 'plugins' | `plugin:${string}`

interface Props {
  activePane: ActivePane
  registry: PluginRegistrySnapshot
  theme: 'light' | 'dark'
  onSelectPane: (pane: ActivePane) => void
  onSelectFiles: () => void
  onSelectChanges: () => void
  onToggleTheme: () => void
  onShowHelp: () => void
}

export function ActivityBar({
  activePane,
  registry,
  theme,
  onSelectPane,
  onSelectFiles,
  onSelectChanges,
  onToggleTheme,
  onShowHelp,
}: Props) {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const settingsRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!settingsOpen) return
    const close = (event: MouseEvent) => {
      if (!settingsRef.current?.contains(event.target as Node)) setSettingsOpen(false)
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [settingsOpen])

  const sidebars = (builtIn: boolean) => registry.sidebars
    .filter(sidebar => registry.plugins.some(plugin => (
      plugin.id === sidebar.pluginId && (plugin.origin === 'built-in') === builtIn
    )))
    .map(sidebar => {
      const pane = `plugin:${sidebar.key}` as const
      return (
        <button
          key={sidebar.key}
          className={`activity-btn${builtIn ? ' activity-brand' : ''}${activePane === pane ? ' active' : ''}`}
          title={sidebar.title}
          aria-label={sidebar.title}
          aria-pressed={activePane === pane}
          onClick={() => onSelectPane(pane)}
        >
          <Icon name={sidebar.icon ?? 'extensions'} size={20} />
        </button>
      )
    })

  return (
    <nav className="activity-bar" role="toolbar" aria-label="活动栏">
      <button
        className={`activity-btn activity-planner${activePane === 'files' ? ' active' : ''}`}
        title="文件"
        aria-label="文件"
        aria-pressed={activePane === 'files'}
        onClick={onSelectFiles}
      >
        <Icon name="folder" size={20} />
      </button>
      <button
        className={`activity-btn${activePane === 'changes' ? ' active' : ''}`}
        title="Git 变更"
        aria-label="Git 变更"
        aria-pressed={activePane === 'changes'}
        onClick={onSelectChanges}
      >
        <Icon name="git-branch" size={20} />
      </button>
      {sidebars(true)}
      <button
        className={`activity-btn activity-engineer${activePane === 'plugins' ? ' active' : ''}`}
        title="插件"
        aria-label="插件"
        aria-pressed={activePane === 'plugins'}
        onClick={() => onSelectPane('plugins')}
      >
        <Icon name="extensions" size={20} />
      </button>
      {sidebars(false)}
      <div className="activity-bar-bottom" ref={settingsRef}>
        <button
          className="activity-btn"
          title={theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'}
          aria-label={theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'}
          onClick={onToggleTheme}
        >
          <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={20} />
        </button>
        <button
          className={`activity-btn${settingsOpen ? ' active' : ''}`}
          title="设置"
          aria-label="设置"
          aria-haspopup="true"
          aria-expanded={settingsOpen}
          onClick={() => setSettingsOpen(open => !open)}
        >
          <Icon name="settings" size={20} />
        </button>
        {settingsOpen && (
          <div className="settings-dropdown" role="menu">
            <button
              className="settings-item"
              role="menuitem"
              onClick={() => {
                onShowHelp()
                setSettingsOpen(false)
              }}
            >
              <Icon name="help" size={14} />
              <span>键盘快捷键 / 帮助</span>
            </button>
          </div>
        )}
      </div>
    </nav>
  )
}
