import { invoke } from '@tauri-apps/api/core'
export interface Incident { id: string; time: string; category: string; message: string; details: string }
let incidents: Incident[] = []
const listeners = new Set<() => void>()
const recordedObjects = new WeakMap<object, string>()
export const incidentSnapshot = () => incidents
export const subscribeIncidents = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener) } }
let queue: { category: string; command?: string; error_id: string }[] = []
let timer: ReturnType<typeof setTimeout> | undefined
let sending = false
export let loggingFailure = false
export function recordIncident(category: string, error: unknown, command?: string, displayMessage?: string) {
  const value = error && typeof error === 'object' ? error as Record<string, unknown> : null
  const id = typeof value?.error_id === 'string' ? value.error_id : crypto.randomUUID()
  const message = displayMessage ?? (typeof value?.message === 'string' ? value.message : String(error))
  // 同一个 IPC 错误经过展示层时只更新摘要，保留第一次捕获的详情与日志关联。
  const previousId = value ? recordedObjects.get(value) : undefined
  if (previousId && incidents.some(incident => incident.id === previousId)) {
    if (displayMessage) {
      incidents = incidents.map(incident => incident.id === previousId ? { ...incident, message: message.slice(0, 4096) } : incident)
      listeners.forEach(listener => listener())
    }
    return
  }
  if (value) recordedObjects.set(value, id)
  let details = message
  try { details = error instanceof Error ? error.stack ?? message : JSON.stringify(error, (_, v) => typeof v === 'bigint' ? String(v) : v) ?? message } catch { /* 仅保留安全可显示的摘要。 */ }
  incidents = [...incidents, { id, time: new Date().toISOString(), category, message: message.slice(0, 4096), details: details.slice(0, 16384) }].slice(-100)
  listeners.forEach(listener => listener())
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  queue = [...queue, { category, command, error_id: id }].slice(-128)
  if (!timer && !sending) timer = setTimeout(() => { timer = undefined; void flush() }, 250)
}
async function flush() {
  if (sending || queue.length === 0) return
  sending = true
  const events = queue.splice(0, 32)
  try { await invoke('record_frontend_events', { events }); loggingFailure = false } catch { loggingFailure = true }
  finally {
    sending = false
    // 日志通道失败不递归上报或无限重试；下一批事件仍可再次尝试。
    if (loggingFailure) queue = []
    else if (queue.length) timer = setTimeout(() => { timer = undefined; void flush() }, 250)
    incidents = [...incidents]
    listeners.forEach(listener => listener())
  }
}
export function installGlobalErrorHandlers() {
  window.addEventListener('error', event => recordIncident('window_error', event.error ?? event.message))
  window.addEventListener('unhandledrejection', event => recordIncident('unhandled_rejection', event.reason))
}
