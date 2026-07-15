import { Icon } from './Icon'
import { useUpdater, type UpdaterState } from '../hooks/useUpdater'

export function UpdateControl() {
  const { state, activate } = useUpdater()
  const busy = state.phase === 'checking'
    || state.phase === 'downloading'
    || state.phase === 'restarting'
  const available = state.phase === 'available'

  return (
    <div className="sidebar-update">
      <button
        className={`sidebar-update-button${available ? ' available' : ''}`}
        type="button"
        onClick={() => void activate()}
        disabled={busy}
        title={updateTitle(state)}
        aria-label={updateTitle(state)}
      >
        <Icon
          name={available ? 'download' : state.phase === 'up-to-date' ? 'check' : 'refresh'}
          size={13}
          className={busy ? 'update-icon-spinning' : undefined}
        />
        <span className="sidebar-version">
          {state.currentVersion ? `v${state.currentVersion}` : 'CFD Editor'}
        </span>
        <span className="sidebar-update-label">{updateLabel(state)}</span>
      </button>
    </div>
  )
}

function updateLabel(state: UpdaterState): string {
  switch (state.phase) {
    case 'checking': return '正在检查'
    case 'up-to-date': return '已是最新'
    case 'available': return state.availableVersion ? `更新至 v${state.availableVersion}` : '下载更新'
    case 'downloading': return state.progress === undefined ? '正在下载' : `正在下载 ${state.progress}%`
    case 'restarting': return '正在重启'
    case 'error': return '重试更新'
    default: return '检查更新'
  }
}

function updateTitle(state: UpdaterState): string {
  if (state.error) return `更新失败：${state.error}`
  if (state.phase === 'available' && state.availableVersion) {
    return `下载并安装 CFD Editor ${state.availableVersion}`
  }
  return updateLabel(state)
}
