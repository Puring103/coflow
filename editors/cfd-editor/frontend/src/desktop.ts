import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'
import { open as tauriOpenDialog } from '@tauri-apps/plugin-dialog'

export const isTauriDesktop = '__TAURI_INTERNALS__' in window
export const isElectronDesktop = typeof window.cfdEditorElectron !== 'undefined'
export const isDesktop = isTauriDesktop || isElectronDesktop

export interface OpenDialogOptions {
  directory?: boolean
  filters?: { name: string; extensions: string[] }[]
}

export async function invokeDesktop<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (window.cfdEditorElectron) {
    return window.cfdEditorElectron.invoke(command, args) as Promise<T>
  }
  return tauriInvoke<T>(command, args)
}

export async function listenDesktop<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (window.cfdEditorElectron) {
    const listenerId = window.cfdEditorElectron.subscribe(event, payload => handler(payload as T))
    return () => window.cfdEditorElectron?.unsubscribe(listenerId)
  }
  return tauriListen<T>(event, message => handler(message.payload))
}

export async function openDesktopDialog(options: OpenDialogOptions): Promise<string | null> {
  if (window.cfdEditorElectron) {
    return window.cfdEditorElectron.openDialog(options)
  }
  const path = await tauriOpenDialog({
    multiple: false,
    directory: options.directory,
    filters: options.filters,
  })
  return typeof path === 'string' ? path : null
}
