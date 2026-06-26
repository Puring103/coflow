import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { CfdValue } from './bindings/CfdValue'
import type { DeleteRecordOutcome } from './bindings/DeleteRecordOutcome'
import type { FileRecords } from './bindings/FileRecords'
import type { GraphData } from './bindings/GraphData'
import type { InsertRecordOutcome } from './bindings/InsertRecordOutcome'
import type { ProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RefTarget } from './bindings/RefTarget'
import type { WriteFieldOutcome } from './bindings/WriteFieldOutcome'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import { fromIpc, toIpc, type FieldPathSegment, type FieldValue } from './wire'

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

export async function pickProjectDirectory(): Promise<string | null> {
  if (!isTauri) {
    alert('文件对话框仅在 Tauri 桌面环境可用。')
    return null
  }
  const path = await openDialog({
    multiple: false,
    directory: true,
  })
  return typeof path === 'string' ? path : null
}

export async function loadProject(yamlPath: string): Promise<ProjectSnapshot> {
  return invokeCommand<ProjectSnapshot>('load_project', { yamlPath })
}

export async function initProject(dir: string): Promise<ProjectSnapshot> {
  return invokeCommand<ProjectSnapshot>('init_project', { dir })
}

export async function getFileRecords(sessionId: number, filePath: string): Promise<FileRecords> {
  return invokeCommand<FileRecords>('get_file_records', { sessionId, filePath })
}

export async function getGraph(sessionId: number, filePath: string): Promise<GraphData> {
  return invokeCommand<GraphData>('get_graph', { sessionId, filePath })
}

export async function closeSession(sessionId: number): Promise<void> {
  return invokeCommand('close_session', { sessionId })
}

export async function getEnumVariants(sessionId: number, enumName: string): Promise<string[]> {
  return invokeCommand<string[]>('get_enum_variants', { sessionId, enumName })
}

export async function getRefTargets(sessionId: number, targetType: string): Promise<RefTarget[]> {
  return invokeCommand<RefTarget[]>('get_ref_targets', { sessionId, targetType })
}

export async function makeDefaultObject(sessionId: number, typeName: string): Promise<FieldValue> {
  return invokeCommand<CfdValue>('make_default_object', { sessionId, typeName })
}

export async function writeField(
  sessionId: number,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
  newValue: FieldValue,
): Promise<WriteFieldOutcome> {
  return invokeCommand<WriteFieldOutcome>('write_field', {
    sessionId,
    coordinate,
    fieldPath,
    newValue,
  })
}

export async function insertRecord(
  sessionId: number,
  filePath: string,
  recordKey: string,
  actualType: string,
  fields: FieldValue,
): Promise<InsertRecordOutcome> {
  return invokeCommand<InsertRecordOutcome>('insert_record', {
    sessionId,
    filePath,
    recordKey,
    actualType,
    fields,
  })
}

export async function deleteRecord(
  sessionId: number,
  coordinate: RecordCoordinate,
): Promise<DeleteRecordOutcome> {
  return invokeCommand<DeleteRecordOutcome>('delete_record', {
    sessionId,
    coordinate,
  })
}

async function invokeCommand<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = await invoke<unknown>(cmd, toIpc(args) as Record<string, unknown>)
  return fromIpc(result) as T
}
