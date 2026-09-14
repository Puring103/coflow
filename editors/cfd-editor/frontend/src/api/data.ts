import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { BatchWriteFieldOutcome } from '../bindings/BatchWriteFieldOutcome'
import type { CfdValue } from '../bindings/CfdValue'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { DeleteRecordOutcome } from '../bindings/DeleteRecordOutcome'
import type { DimensionValueCoordinate } from '../bindings/DimensionValueCoordinate'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import type { DimensionValueView } from '../bindings/DimensionValueView'
import type { EnumVariantOption } from '../bindings/EnumVariantOption'
import type { FileRecords } from '../bindings/FileRecords'
import type { GraphData } from '../bindings/GraphData'
import type { InsertRecordOutcome } from '../bindings/InsertRecordOutcome'
import type { PluginSchemaType } from '../bindings/PluginSchemaType'
import type { ProjectSearchMode } from '../bindings/ProjectSearchMode'
import type { ProjectSearchResults } from '../bindings/ProjectSearchResults'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { RefTarget } from '../bindings/RefTarget'
import type { RenameRecordOutcome } from '../bindings/RenameRecordOutcome'
import type { ReorderRecordsOutcome } from '../bindings/ReorderRecordsOutcome'
import type { WriteDimensionValueOutcome } from '../bindings/WriteDimensionValueOutcome'
import type { WriteFieldOutcome } from '../bindings/WriteFieldOutcome'
import type { FieldPathSegment, FieldValue } from '../wire'
import { invokeCommand } from './tauriEnv'

export async function getFileRecords(sessionId: number, filePath: string): Promise<FileRecords> {
  return invokeCommand<FileRecords>('get_file_records', { sessionId, filePath })
}

export async function searchRecords(
  sessionId: number,
  query: string,
  mode: ProjectSearchMode,
  limit = 200,
): Promise<ProjectSearchResults> {
  return invokeCommand<ProjectSearchResults>('search_records', { sessionId, query, mode, limit })
}

export async function getPluginSchema(sessionId: number): Promise<PluginSchemaType[]> {
  return invokeCommand<PluginSchemaType[]>('get_plugin_schema', { sessionId })
}

export async function getPluginRecordsByType(sessionId: number, typeName: string): Promise<RecordRow[]> {
  return invokeCommand<RecordRow[]>('get_plugin_records_by_type', { sessionId, typeName })
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

export async function getEnumVariants(sessionId: number, enumName: string): Promise<EnumVariantOption[]> {
  const variants = await invokeCommand<EnumVariantOption[]>('get_enum_variants', { sessionId, enumName })
  return variants.map(variant => ({ ...variant, value: BigInt(variant.value) }))
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

export async function writeFields(
  sessionId: number,
  writes: BatchWriteFieldInput[],
): Promise<BatchWriteFieldOutcome> {
  return invokeCommand<BatchWriteFieldOutcome>('write_fields', {
    sessionId,
    writes,
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

export async function swapRecords(
  sessionId: number,
  first: RecordCoordinate,
  second: RecordCoordinate,
): Promise<ReorderRecordsOutcome> {
  return invokeCommand<ReorderRecordsOutcome>('swap_records', { sessionId, first, second })
}

export async function moveRecord(
  sessionId: number,
  coordinate: RecordCoordinate,
  targetIndex: number,
): Promise<ReorderRecordsOutcome> {
  return invokeCommand<ReorderRecordsOutcome>('move_record', {
    sessionId,
    coordinate,
    targetIndex,
  })
}

export async function transferRecord(
  sessionId: number,
  coordinate: RecordCoordinate,
  destinationFile: string,
  targetIndex: number,
): Promise<ReorderRecordsOutcome> {
  return invokeCommand<ReorderRecordsOutcome>('transfer_record', {
    sessionId,
    coordinate,
    destinationFile,
    targetIndex,
  })
}

