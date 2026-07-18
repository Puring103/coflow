import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { CfdValue } from './bindings/CfdValue'
import type { CollectionEdit } from './bindings/CollectionEdit'
import type { CreateRecordDraft } from './bindings/CreateRecordDraft'
import type { DeleteRecordOutcome } from './bindings/DeleteRecordOutcome'
import type { DimensionValueCoordinate } from './bindings/DimensionValueCoordinate'
import type { DimensionInfo } from './bindings/DimensionInfo'
import type { DimensionValueState } from './bindings/DimensionValueState'
import type { DimensionValueView } from './bindings/DimensionValueView'
import type { FileRecords } from './bindings/FileRecords'
import type { EditorProjectSettings } from './bindings/EditorProjectSettings'
import type { EditorRecordGroup } from './bindings/EditorRecordGroup'
import type { GraphData } from './bindings/GraphData'
import type { InsertRecordOutcome } from './bindings/InsertRecordOutcome'
import type { ProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RefTarget } from './bindings/RefTarget'
import type { RenameRecordOutcome } from './bindings/RenameRecordOutcome'
import type { WriteFieldOutcome } from './bindings/WriteFieldOutcome'
import type { WriteDimensionValueOutcome } from './bindings/WriteDimensionValueOutcome'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import { fromIpc, toIpc, type FieldPathSegment, type FieldValue } from './wire'

export const isTauri = '__TAURI_INTERNALS__' in window

export interface ProjectChangedEvent {
  session_id: number
  changed_paths: string[]
  snapshot: ProjectSnapshot
}

export interface ProjectWatchErrorEvent {
  session_id: number
  message: string
}

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

export interface GraphQueryOptions {
  depth?: number
  limit?: number
}

export async function getGraph(
  sessionId: number,
  filePath: string,
  options: GraphQueryOptions = {},
): Promise<GraphData> {
  return invokeCommand<GraphData>('get_graph', {
    sessionId,
    filePath,
    depth: options.depth ?? null,
    limit: options.limit ?? null,
  })
}

export async function closeSession(sessionId: number): Promise<void> {
  return invokeCommand('close_session', { sessionId })
}

export interface DimensionFileRow {
  coordinate: RecordCoordinate
  default_value: FieldValue
  values: Record<string, DimensionValueState | undefined>
}

export interface DimensionFileRecords {
  revision: number
  file_path: string
  dimension: string
  display_name: string
  field: string
  variants: string[]
  rows: DimensionFileRow[]
}

export async function getProjectSettings(sessionId: number): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('get_project_settings', { sessionId })
}

export async function getProjectDimensions(sessionId: number): Promise<DimensionInfo[]> {
  return invokeCommand<DimensionInfo[]>('get_project_dimensions', { sessionId })
}

export async function getDimensionFileRecords(
  sessionId: number,
  filePath: string,
): Promise<DimensionFileRecords> {
  return invokeCommand<DimensionFileRecords>('get_dimension_file_records', { sessionId, filePath })
}

export async function setTableColumnWidths(
  sessionId: number,
  filePath: string,
  actualType: string,
  widths: Record<string, number>,
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_table_column_widths', {
    sessionId,
    filePath,
    actualType,
    widths,
  })
}

export async function setRecordGroups(
  sessionId: number,
  filePath: string,
  actualType: string,
  groups: EditorRecordGroup[],
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_record_groups', {
    sessionId,
    filePath,
    actualType,
    groups,
  })
}

export async function checkProject(sessionId: number): Promise<string> {
  return invokeCommand<string>('check_project', { sessionId })
}

export async function buildProject(sessionId: number): Promise<string> {
  return invokeCommand<string>('build_project', { sessionId })
}

export async function openSourceFile(sessionId: number, filePath: string): Promise<void> {
  return invokeCommand('open_source_file', { sessionId, filePath })
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

export async function createRecordDraft(sessionId: number, actualType: string): Promise<CreateRecordDraft> {
  return invokeCommand<CreateRecordDraft>('create_record_draft', { sessionId, actualType })
}

export async function renderCellText(
  sessionId: number,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
): Promise<string> {
  return invokeCommand<string>('render_cell_text', { sessionId, coordinate, fieldPath })
}

export async function parseCellText(
  sessionId: number,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
  text: string,
): Promise<FieldValue> {
  return invokeCommand<CfdValue>('parse_cell_text', { sessionId, coordinate, fieldPath, text })
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

export async function getDimensionValue(
  sessionId: number,
  coordinate: DimensionValueCoordinate,
): Promise<DimensionValueView> {
  return invokeCommand<DimensionValueView>('get_dimension_value', { sessionId, coordinate })
}

export async function writeDimensionValue(
  sessionId: number,
  coordinate: DimensionValueCoordinate,
  expectedValue: DimensionValueState,
  newValue: DimensionValueState,
): Promise<WriteDimensionValueOutcome> {
  return invokeCommand<WriteDimensionValueOutcome>('write_dimension_value', {
    sessionId,
    coordinate,
    expectedValue,
    newValue,
  })
}

export async function editCollection(
  sessionId: number,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
  edit: CollectionEdit,
): Promise<WriteFieldOutcome> {
  return invokeCommand<WriteFieldOutcome>('edit_collection', {
    sessionId,
    coordinate,
    fieldPath,
    edit,
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

export async function renameRecordKey(
  sessionId: number,
  coordinate: RecordCoordinate,
  newKey: string,
): Promise<RenameRecordOutcome> {
  return invokeCommand<RenameRecordOutcome>('rename_record_key', {
    sessionId,
    coordinate,
    newKey,
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

export async function onProjectChanged(handler: (event: ProjectChangedEvent) => void): Promise<() => void> {
  return listen<ProjectChangedEvent>('project_changed', event => handler(fromIpc(event.payload) as ProjectChangedEvent))
}

export async function onProjectWatchError(handler: (event: ProjectWatchErrorEvent) => void): Promise<() => void> {
  return listen<ProjectWatchErrorEvent>('project_watch_error', event => handler(fromIpc(event.payload) as ProjectWatchErrorEvent))
}

async function invokeCommand<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = await invoke<unknown>(cmd, toIpc(args) as Record<string, unknown>)
  return fromIpc(result) as T
}
