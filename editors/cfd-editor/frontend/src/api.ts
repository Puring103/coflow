import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { ProjectSnapshot, FileRecords, RecordRow, GraphData } from './bindings/index'

export const isTauri = '__TAURI_INTERNALS__' in window

export async function pickProjectYaml(): Promise<string | null> {
  if (!isTauri) {
    alert('文件对话框仅在 Tauri 桌面环境可用，浏览器中请使用 mock 数据。')
    return null
  }
  const path = await openDialog({
    multiple: false,
    filters: [{ name: 'Coflow Project', extensions: ['yaml', 'yml'] }],
  })
  return typeof path === 'string' ? path : null
}

export async function loadProject(yamlPath: string): Promise<ProjectSnapshot> {
  return invoke<ProjectSnapshot>('load_project', { yamlPath })
}

export async function getFileRecords(sessionId: number, filePath: string): Promise<FileRecords> {
  return invoke<FileRecords>('get_file_records', { sessionId, filePath })
}

export async function getRecord(sessionId: number, filePath: string, recordKey: string): Promise<RecordRow> {
  return invoke<RecordRow>('get_record', { sessionId, filePath, recordKey })
}

export async function getGraph(sessionId: number, filePath: string): Promise<GraphData> {
  return invoke<GraphData>('get_graph', { sessionId, filePath })
}

export async function closeSession(sessionId: number): Promise<void> {
  return invoke('close_session', { sessionId })
}
