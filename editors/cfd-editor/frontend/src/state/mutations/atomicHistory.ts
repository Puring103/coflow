import type { MutationOp } from '../../bindings/MutationOp'
import type { EditEntry } from '../editorState'
import { committed } from '../editorState'
import { replayEntry, type ReplayDriver } from './historyReplay'

/** 复用同一份逆操作规则，只收集请求，最终交给引擎一次发布。 */
export async function historyOperations(entry: EditEntry, direction: 'undo' | 'redo'): Promise<MutationOp[] | null> {
  const operations: MutationOp[] = []
  const push = async (operation: MutationOp) => { operations.push(operation); return committed(undefined) }
  const driver: ReplayDriver = {
    applyGraphPositions: () => undefined,
    writeField: (file, record, path, value) => push({ op: 'set_field', file, record, path, value: { kind: 'cfd', value } }),
    writeFields: async (file, writes) => { for (const item of writes) await driver.writeField(file, item.coordinate, item.field_path, item.new_value); return committed(undefined) },
    writeDimensionValue: (file, coordinate, expectedValue, next) => {
      const expected = expectedValue.kind === 'missing' ? { kind: 'missing' as const } : { kind: 'value' as const, value: { kind: 'cfd' as const, value: expectedValue.value } }
      return push(next.kind === 'missing'
        ? { op: 'clear_dimension_value', coordinate, expected }
        : { op: 'set_dimension_value', coordinate, expected, file: file.startsWith('@dimension/') ? file.slice('@dimension/'.length).split('/').slice(1).join('/') : file, value: { kind: 'cfd', value: next.value } })
    },
    insertRecord: (file, id, type, fields) => {
      if (fields.kind !== 'object') throw new Error('记录快照不是对象')
      return push({ op: 'insert_record', file, id, type, fields: { kind: 'cfd', value: fields.value.fields }, materialization: 'minimal' })
    },
    deleteRecord: (file, record) => push({ op: 'delete_record', file, record }),
    swapRecords: (file, first, second) => push({ op: 'swap_records', file, first, second }),
    moveRecord: (file, record, target_index) => push({ op: 'move_record', file, record, target_index }),
    transferRecord: (source_file, destination_file, record, target_index) => push({ op: 'transfer_record', source_file, destination_file, record, target_index }),
  }
  async function collect(item: EditEntry): Promise<boolean> {
    if (item.kind === 'graph-layout') return false
    if (item.kind === 'batch') {
      for (const child of direction === 'undo' ? [...item.entries].reverse() : item.entries) if (!await collect(child)) return false
      return true
    }
    return (await replayEntry(driver, item, direction)).status === 'committed'
  }
  return await collect(entry) ? operations : null
}
