import type { FileRecords } from '../bindings/FileRecords'
import type { EditorLookupBackend } from '../features/workspace/state/editorLookups'
import { makeObjectValue } from '../wire'
import { summaryOf } from '../features/record-editing/value/fieldValue'
import { MOCK_FILE_RECORDS } from './mock'

/** 浏览器示例使用明确的示例枚举与实时记录缓存，不调用桌面 IPC。 */
export function createMockLookups(files: () => Record<string, FileRecords>): EditorLookupBackend {
  const sample = (type: string) => {
    const record = Object.values(MOCK_FILE_RECORDS).flatMap(file => file.records).find(row => row.coordinate.actual_type === type)
    if (!record) throw new Error(`示例中没有类型 ${type}`)
    return structuredClone(record)
  }
  return {
    async getEnumVariants(_session, name) {
      const variants: Record<string, string[]> = { Quality: ['Common', 'Uncommon', 'Rare'], Faction: ['Neutral', 'Friendly', 'Hostile'] }
      if (!variants[name]) throw new Error(`示例中没有枚举 ${name}`)
      return variants[name].map((variant, index) => ({ name: variant, value: BigInt(index), label: null, description: null }))
    },
    async getRefTargets(_session, type) {
      return Object.values(files()).flatMap(file => file.records.filter(row => row.coordinate.actual_type === type || (type === 'Item' && row.coordinate.actual_type === 'Weapon')).map(row => ({
        coordinate: row.coordinate, file_path: file.file_path,
        short_name: row.fields.find(field => field.name === 'name') ? summaryOf(row.fields.find(field => field.name === 'name')!.value) : null,
      })))
    },
    async makeDefaultObject(_session, type) { return makeObjectValue(type, sample(type).fields) },
    async createRecordDraft(_session, type) {
      return { actual_type: type, fields: sample(type).fields.map(field => ({ name: field.name, value: field.value, annotation: field.annotation, source: 'type_default' as const, required: null })) }
    },
  }
}
