import { Menu } from '../../../shared/ui/Menu'
import { Icon } from '../../../shared/ui/Icon'

export function EditorSettingsMenu({ theme, onToggleTheme, onShowHelp }: {
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  onShowHelp: () => void
}) {
  return <Menu className="editor-settings" title="设置" label={<Icon name="gear" size={17} />}>
    <span className="editor-settings-heading">设置</span>
    <button className="btn" onClick={onToggleTheme}><Icon name={theme === 'dark' ? 'sun' : 'moon'} size={15} />{theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'}</button>
    <button className="btn" onClick={onShowHelp}><Icon name="help" size={15} />快捷键与帮助</button>
  </Menu>
}
