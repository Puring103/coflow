import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { CfdValue as WireCfdValue } from './bindings/CfdValue'
import type { DeleteRecordOutcome as WireDeleteRecordOutcome } from './bindings/DeleteRecordOutcome'
import type { FileRecords as WireFileRecords } from './bindings/FileRecords'
import type { GraphData as WireGraphData } from './bindings/GraphData'
import type { InsertRecordOutcome as WireInsertRecordOutcome } from './bindings/InsertRecordOutcome'
import type { ProjectSnapshot as WireProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RefTarget } from './bindings/RefTarget'
import type { WriteFieldOutcome as WireWriteFieldOutcome } from './bindings/WriteFieldOutcome'
import {
  cfdValueFromWire,
  cfdValueToWire,
  deleteRecordOutcomeFromWire,
  fieldPathSegmentToWire,
  fileRecordsFromWire,
  graphFromWire,
  insertRecordOutcomeFromWire,
  projectSnapshotFromWire,
  writeFieldOutcomeFromWire,
  type DeleteRecordOutcome,
  type FieldPathSegment,
  type FieldValue,
  type FileRecords,
  type GraphData,
  type InsertRecordOutcome,
  type ProjectSnapshot,
  type WriteFieldOutcome,
} from './bindings/index'

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
  const wire = await invoke<WireProjectSnapshot>('load_project', { yamlPath })
  return projectSnapshotFromWire(wire)
}

export async function initProject(dir: string): Promise<ProjectSnapshot> {
  const wire = await invoke<WireProjectSnapshot>('init_project', { dir })
  return projectSnapshotFromWire(wire)
}

export async function getFileRecords(sessionId: number, filePath: string): Promise<FileRecords> {
  const wire = await invoke<WireFileRecords>('get_file_records', { sessionId, filePath })
  return fileRecordsFromWire(wire)
}

export async function getGraph(sessionId: number, filePath: string): Promise<GraphData> {
  const wire = await invoke<WireGraphData>('get_graph', { sessionId, filePath })
  return graphFromWire(wire)
}

export async function closeSession(sessionId: number): Promise<void> {
  return invoke('close_session', { sessionId })
}

export async function getEnumVariants(sessionId: number, enumName: string): Promise<string[]> {
  return invoke<string[]>('get_enum_variants', { sessionId, enumName })
}

export async function getRefTargets(sessionId: number, targetType: string): Promise<string[]> {
  const targets = await invoke<RefTarget[]>('get_ref_targets', { sessionId, targetType })
  return targets.map((t) => t.coordinate.key)
}

export async function makeDefaultObject(sessionId: number, typeName: string): Promise<FieldValue> {
  const wire = await invoke<WireCfdValue>('make_default_object', { sessionId, typeName })
  return cfdValueFromWire(wire)
}

export async function writeField(
  sessionId: number,
  filePath: string,
  recordKey: string,
  fieldPath: FieldPathSegment[],
  newValue: FieldValue,
  actualType?: string,
): Promise<WriteFieldOutcome> {
  const coordinate = await resolveCoordinate(sessionId, filePath, recordKey, actualType)
  const wire = await invoke<WireWriteFieldOutcome>('write_field', {
    sessionId,
    filePath,
    coordinate,
    fieldPath: fieldPath.map(fieldPathSegmentToWire),
    newValue: cfdValueToWire(newValue),
  })
  return writeFieldOutcomeFromWire(wire)
}

export async function insertRecord(
  sessionId: number,
  filePath: string,
  recordKey: string,
  actualType: string,
  fields: FieldValue,
): Promise<InsertRecordOutcome> {
  const wire = await invoke<WireInsertRecordOutcome>('insert_record', {
    sessionId,
    filePath,
    recordKey,
    actualType,
    fields: cfdValueToWire(fields),
  })
  return insertRecordOutcomeFromWire(wire)
}

export async function deleteRecord(
  sessionId: number,
  filePath: string,
  recordKey: string,
  actualType?: string,
): Promise<DeleteRecordOutcome> {
  const coordinate = await resolveCoordinate(sessionId, filePath, recordKey, actualType)
  const wire = await invoke<WireDeleteRecordOutcome>('delete_record', {
    sessionId,
    filePath,
    coordinate,
  })
  return deleteRecordOutcomeFromWire(wire)
}

/**
 * Compute the wire `RecordCoordinate` from `(file_path, recordKey)` plus an
 * optional explicit `actualType`. When the caller doesn't know the type
 * (legacy code paths, mock data) we re-fetch the file's records and pick
 * the row matching `recordKey`. Cheap and correct; the front-end already
 * caches `FileRecords` by file so the round-trip rarely hits the wire.
 */
async function resolveCoordinate(
  sessionId: number,
  filePath: string,
  recordKey: string,
  actualType: string | undefined,
): Promise<{ actual_type: string; key: string }> {
  if (actualType) {
    return { actual_type: actualType, key: recordKey }
  }
  const records = await getFileRecords(sessionId, filePath)
  const row = records.records.find((r) => r.key === recordKey)
  if (!row) {
    throw new Error(`record \`${recordKey}\` not found in \`${filePath}\``)
  }
  return { actual_type: row.actual_type, key: recordKey }
}
