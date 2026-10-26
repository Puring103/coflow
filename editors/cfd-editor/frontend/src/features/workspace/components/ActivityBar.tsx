import { useRef } from 'react'
import { useSelectionSlide } from '../../../shared/motion/useSelectionSlide'
import type { PluginRegistrySnapshot } from '../../plugins/runtime'
import { Icon } from '../../../shared/ui/Icon'

export type ActivePane = 'files' | 'changes' | 'plugins' | `plugin:${string}`

interface Props {
  activePane: ActivePane
  registry: PluginRegistrySnapshot
  onSelectPane: (pane: ActivePane) => void
  onSelectFiles: () => void
  onSelectChanges: () => void
}

export function ActivityBar({
  activePane,
  registry,
  onSelectPane,
  onSelectFiles,
  onSelectChanges,
}: Props) {
  const root = useRef<HTMLElement>(null)
  useSelectionSlide(root, '.activity-btn.active')
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
    <nav ref={root} className="activity-bar" role="toolbar" aria-label="活动栏">
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
    </nav>
  )
}
