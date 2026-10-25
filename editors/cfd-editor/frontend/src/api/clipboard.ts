import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
import { isTauri } from './tauriEnv'

// 桌面端直接使用系统剪贴板；浏览器预览仍遵循浏览器自身的权限规则。
export function readClipboardText(): Promise<string> {
  return isTauri ? readText() : navigator.clipboard.readText()
}

export function writeClipboardText(text: string): Promise<void> {
  return isTauri ? writeText(text) : navigator.clipboard.writeText(text)
}
