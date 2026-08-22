/// <reference types="vite/client" />

interface CfdEditorElectronBridge {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>
  subscribe(event: string, handler: (payload: unknown) => void): number
  unsubscribe(listenerId: number): void
  openDialog(options: {
    directory?: boolean
    filters?: { name: string; extensions: string[] }[]
  }): Promise<string | null>
}

interface Window {
  cfdEditorElectron?: CfdEditorElectronBridge
}
