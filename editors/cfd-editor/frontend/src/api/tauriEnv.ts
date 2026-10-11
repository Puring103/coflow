import { invoke } from '@tauri-apps/api/core'
import { fromIpc, toIpc } from '../wire'

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export async function invokeCommand<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = await invoke<unknown>(cmd, toIpc(args) as Record<string, unknown>)
  return fromIpc(result) as T
}
