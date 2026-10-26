import { recordIncident } from '../features/diagnostics/incidentReporter'
import { invoke } from '@tauri-apps/api/core'
import { fromIpc, toIpc } from '../wire'

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export async function invokeCommand<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    const result = await invoke<unknown>(cmd, toIpc(args) as Record<string, unknown>)
    return fromIpc(result) as T
  } catch (error) {
    recordIncident("ipc", error, cmd)
    throw error
  }
}
