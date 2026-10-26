import type { RecordRow } from '../../../bindings/RecordRow'
import { coordinateId } from '../../../wire'

// 不可变快照共享索引，批量填充/粘贴避免每个单元格重新扫描整张表。
const recordIndexes = new WeakMap<readonly RecordRow[], ReadonlyMap<string, RecordRow>>()
export function recordIndex(rows: readonly RecordRow[]): ReadonlyMap<string, RecordRow> {
  let index = recordIndexes.get(rows)
  if (!index) {
    index = new Map(rows.map(row => [coordinateId(row.coordinate), row]))
    recordIndexes.set(rows, index)
  }
  return index
}

