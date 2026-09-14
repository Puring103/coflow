import type { BatchWriteFieldInput } from '../../bindings/BatchWriteFieldInput'
import type { DimensionValueCoordinate } from '../../bindings/DimensionValueCoordinate'
import type { DimensionValueState } from '../../bindings/DimensionValueState'
import type { RecordCoordinate } from '../../bindings/RecordCoordinate'
import type { FieldPathSegment, FieldValue } from '../../wire'
import { cloneValue } from '../../wire'
import type {
  BatchEditEntry,
  EditEntry,
  MutationResult,
} from '../editorState'
import { failed } from '../editorState'

/** 回放驱动：控制器内部写入方法的最小端口，undo/redo 只经此表分发。 */
export interface ReplayDriver {
  applyGraphPositions(viewKey: string, positions: import('../editorState').GraphPositions): Promise<MutationResult<unknown>> | undefined
  writeDimensionValue(
    filePath: string,
    coordinate: DimensionValueCoordinate,
    expectedValue: DimensionValueState,
    newValue: DimensionValueState,
  ): Promise<MutationResult<unknown>>
  writeField(
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ): Promise<MutationResult<unknown>>
  writeFields(
    filePath: string,
    writes: BatchWriteFieldInput[],
  ): Promise<MutationResult<unknown>>
  deleteRecord(filePath: string, coordinate: RecordCoordinate): Promise<MutationResult<unknown>>
  insertRecord(
    filePath: string,
    recordKey: string,
    actualType: string,
    fields: FieldValue,
  ): Promise<MutationResult<unknown>>
  swapRecords(filePath: string, first: RecordCoordinate, second: RecordCoordinate): Promise<MutationResult<unknown>>
  moveRecord(filePath: string, coordinate: RecordCoordinate, targetIndex: number): Promise<MutationResult<unknown>>
  transferRecord(
    sourceFile: string,
    destinationFile: string,
    coordinate: RecordCoordinate,
    targetIndex: number,
  ): Promise<MutationResult<unknown>>
}

export type ReplayDirection = 'undo' | 'redo'

type Handler = (driver: ReplayDriver, entry: never, direction: ReplayDirection) => Promise<MutationResult<unknown>>

/** undo/redo 分发表：同一 kind 的两个方向只差取值方向，原 70+70 行镜像分支收敛于此。 */
const REPLAY_TABLE: Record<EditEntry['kind'], Handler> = {
  'graph-layout': (driver, entry, direction) => {
    const layout = entry as Extract<EditEntry, { kind: 'graph-layout' }>
    const positions = direction === 'undo' ? layout.oldPositions : layout.newPositions
    return driver.applyGraphPositions(layout.viewKey, positions) ?? Promise.resolve(failed())
  },
  dimension: (driver, entry, direction) => {
    const dim = entry as Extract<EditEntry, { kind: 'dimension' }>
    const [expected, next] = direction === 'undo'
      ? [dim.newValue, dim.oldValue]
      : [dim.oldValue, dim.newValue]
    return driver.writeDimensionValue(dim.filePath, dim.coordinate, expected, next)
  },
  field: (driver, entry, direction) => {
    const field = entry as Extract<EditEntry, { kind: 'field' }>
    return driver.writeField(
      field.filePath,
      field.coordinate,
      field.fieldPath,
      direction === 'undo' ? field.oldValue : field.newValue,
    )
  },
  'batch-field': (driver, entry, direction) => {
    const batch = entry as Extract<EditEntry, { kind: 'batch-field' }>
    return driver.writeFields(
      batch.edits[0]?.filePath ?? '',
      batch.edits.map(edit => ({
        coordinate: edit.coordinate,
        field_path: edit.fieldPath,
        new_value: cloneValue(direction === 'undo' ? edit.oldValue : edit.newValue),
      })),
    )
  },
  insert: (driver, entry, direction) => {
    const insert = entry as Extract<EditEntry, { kind: 'insert' }>
    return direction === 'undo'
      ? driver.deleteRecord(insert.filePath, insert.coordinate)
      : driver.insertRecord(insert.filePath, insert.coordinate.key, insert.coordinate.actual_type, insert.fields)
  },
  delete: (driver, entry, direction) => {
    const deleted = entry as Extract<EditEntry, { kind: 'delete' }>
    return direction === 'undo'
      ? driver.insertRecord(deleted.filePath, deleted.coordinate.key, deleted.coordinate.actual_type, deleted.snapshot)
      : driver.deleteRecord(deleted.filePath, deleted.coordinate)
  },
  'swap-records': (driver, entry) => {
    const swap = entry as Extract<EditEntry, { kind: 'swap-records' }>
    return driver.swapRecords(swap.filePath, swap.first, swap.second)
  },
  'move-record': (driver, entry, direction) => {
    const move = entry as Extract<EditEntry, { kind: 'move-record' }>
    return driver.moveRecord(
      move.filePath,
      move.coordinate,
      direction === 'undo' ? move.oldIndex : move.newIndex,
    )
  },
  'transfer-record': (driver, entry, direction) => {
    const transfer = entry as Extract<EditEntry, { kind: 'transfer-record' }>
    return direction === 'undo'
      ? driver.transferRecord(transfer.destinationFile, transfer.filePath, transfer.coordinate, transfer.sourceIndex)
      : driver.transferRecord(transfer.filePath, transfer.destinationFile, transfer.coordinate, transfer.targetIndex)
  },
  batch: () => {
    throw new Error('batch entries are expanded by the caller')
  },
}

export function replayEntry(
  driver: ReplayDriver,
  entry: EditEntry,
  direction: ReplayDirection,
): Promise<MutationResult<unknown>> {
  return REPLAY_TABLE[entry.kind](driver, entry as never, direction)
}

/** 批量条目展开：undo 逆序、redo 顺序，短路于首个非提交结果。 */
export async function replayBatch(
  entry: BatchEditEntry,
  direction: ReplayDirection,
  replay: (sub: EditEntry) => Promise<MutationResult<unknown>>,
): Promise<MutationResult<unknown>> {
  const entries = direction === 'undo' ? [...entry.entries].reverse() : entry.entries
  let result: MutationResult<unknown> = { status: 'committed', value: undefined }
  for (const sub of entries) {
    result = await replay(sub)
    if (result.status !== 'committed') break
  }
  return result
}
