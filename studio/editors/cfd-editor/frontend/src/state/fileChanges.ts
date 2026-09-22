import type { FileRecords } from '../bindings/FileRecords'
import type { FileRecordsPatch } from '../bindings/FileRecordsPatch'
import { coordinateId } from '../wire'

/** 按后端顺序发布行增量；未变化行保留引用，删除不依赖前端推断。 */
export function applyFileRecordsPatch(
  previous: FileRecords | undefined,
  patch: FileRecordsPatch,
  baseRevision: number,
): FileRecords | undefined {
  if (!previous || previous.revision !== baseRevision) return undefined
  const rows = new Map(previous.records.map(row => [coordinateId(row.coordinate), row]))
  for (const row of patch.data.records) rows.set(coordinateId(row.coordinate), row)
  const records = patch.order.map(coordinate => rows.get(coordinateId(coordinate)))
  if (records.some(row => !row)) return undefined
  return {
    ...patch.data,
    records: records.map((row, index) => {
      const value = row!
      return value.container_index === index && value.container_size === records.length
        ? value
        : { ...value, container_index: index, container_size: records.length }
    }),
  }
}
