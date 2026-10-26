export type HistoryShortcut = 'undo' | 'redo'

export function historyShortcutFor(event: KeyboardEvent): HistoryShortcut | null {
  if (!event.metaKey && !event.ctrlKey) return null
  if (usesNativeTextHistory(event.target)) return null
  const key = event.key.toLowerCase()
  if (key === 'z') return event.shiftKey ? 'redo' : 'undo'
  if (key === 'y') return 'redo'
  return null
}

function usesNativeTextHistory(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable || target.tagName === 'TEXTAREA') return true
  if (target.tagName !== 'INPUT') return false
  const type = (target as HTMLInputElement).type.toLowerCase()
  return type === ''
    || type === 'text'
    || type === 'search'
    || type === 'email'
    || type === 'url'
    || type === 'tel'
    || type === 'password'
    || type === 'number'
}
