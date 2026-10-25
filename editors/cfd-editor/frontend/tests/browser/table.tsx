import { EnumDirectSelect } from '../../src/components/DataCard'
import type { FieldValue } from '../../src/wire'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TableView } from '../../src/components/TableView'
import { DimensionCellEditor, DimensionTableView } from '../../src/components/DimensionTableView'
import { MOCK_FILE_RECORDS } from '../../src/mock'
import { createMockLookups } from '../../src/mockLookups'
import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { EditorLookupController } from '../../src/state/editorLookups'
import type { DimensionFileRow, DimensionFileRecords } from '../../src/api'
import { updateRecordSelection, updateValueSelection, type EditorSelection } from '../../src/state/editorSelection'
import { plainFieldValueText } from '../../src/value/fieldValue'
import '../../src/style.css'

// 使用真实表格和异步快照更新，覆盖保存期间的 DOM 身份与焦点行为。
const original = MOCK_FILE_RECORDS['data/item.cfd']
const seed = structuredClone(original.records[0])
if (new URLSearchParams(location.search).has('complex')) {
  seed.field_index.Config = seed.fields.length
  seed.fields.push({ name: 'Config', missing: false, annotation: null, value: {
    kind: 'object', value: { actual_type: 'Config', fields: {
      Core: { kind: 'object', value: { actual_type: 'CoreConfig', fields: { Nested: { kind: 'int', value: 1n } } } },
      Count: { kind: 'int', value: 64n },
    } },
  } })
}
const fixture = {
  ...original,
  columns: new URLSearchParams(location.search).has('complex')
    ? [...original.columns, { ...original.columns[0], name: 'Config' }] : original.columns,
  records: Array.from({ length: Math.min(10000, Math.max(100, Number(new URLSearchParams(location.search).get('rows')) || 100)) }, (_, index) => ({
    ...structuredClone(seed),
    coordinate: { ...seed.coordinate, id: `row_${index}` },
    container_index: index,
    fields: seed.fields.map(field => field.name === 'name' ? {
      ...field, value: { kind: 'string' as const, value: index === 2 ? '这是一段用于验证长文本展开的字符串。'.repeat(14) : `文本 ${index}` },
    } : structuredClone(field)),
  })),
}

const control = { writes: 0, batches: 0, fail: false, delay: 40, selectionEvents: [] as string[], writtenKinds: [] as string[] }
Object.assign(window, { tableTest: control })
const lookupBackend = createMockLookups(() => MOCK_FILE_RECORDS)
const getEnumVariants = lookupBackend.getEnumVariants
lookupBackend.getEnumVariants = async (session, name) => name === 'Permissions'
  ? ['None', 'Read', 'Write', 'Execute'].map((name, index) => ({ name, value: index === 0 ? 0n : 1n << BigInt(index - 1), label: null, description: null }))
  : getEnumVariants(session, name)
const lookups = new EditorLookupController(lookupBackend)
lookups.adopt({ sessionId: 1, revision: 1 })

function Fixture() {
  const [data, setData] = useState(fixture)
  const [flags, setFlags] = useState<FieldValue & {kind: 'enum' | 'option_none'}>({ kind: 'enum', value: { enum_name: 'Permissions', variant: null, value: 0n } })
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [dimension, setDimension] = useState<DimensionFileRow>({
    coordinate: seed.coordinate, field: 'name', owner_file_path: data.file_path,
    default_value: { kind: 'string', value: '基础文本' }, values: {}, sources: {},
  })
  const dimensionCount = Number(new URLSearchParams(location.search).get('dimensionRows')) || 0
  const [dimensionData, setDimensionData] = useState<DimensionFileRecords>(() => ({
    revision: 1, file_path: '@dimension/language/localization/en.cfd', dimension: 'language', display_name: '语言', variants: ['en', 'fr'],
    rows: Array.from({ length: dimensionCount }, (_, index) => ({ ...dimension, coordinate: { ...seed.coordinate, id: `row_${index}` } })),
  }))
  if (dimensionCount) return <div style={{ height: 480, width: 880, display: 'flex' }}><DimensionTableView data={dimensionData} mode="table"
    onWrite={async (row, variant, _expected, next) => setDimensionData(current => ({ ...current,
      rows: current.rows.map(item => item.coordinate.id === row.coordinate.id ? { ...item, values: { ...item.values, [variant]: next } } : item),
    }))} /></div>
  return <>
    <div style={{ height: 480, width: 880 }}>
      <TableView data={data} activeType="Item" selection={selection}
        visibleColumns={new URLSearchParams(location.search).has('complex') ? ['name', 'Config'] : undefined}
        onOpenRecord={() => {}}
        onSelectValue={(coordinate, path, mode = 'replace') => {
          control.selectionEvents.push(`${coordinate.id}:${path[0]?.value}`)
          setSelection(current => updateValueSelection(current, data.file_path, coordinate, path, mode))
        }}
        onSelectRecord={(coordinate, mode, visible) => {
          control.selectionEvents.push(`record:${coordinate.id}`)
          setSelection(current => updateRecordSelection(current, data.file_path, coordinate, visible, mode))
        }}
        onClearSelection={() => setSelection(null)}
        onRenderCellText={async (coordinate, path) => {
          const value = data.records.find(record => record.coordinate.id === coordinate.id)!.fields.find(field => field.name === path[0].value)!.value
          return value.kind === 'string' ? JSON.stringify(value.value) : value.kind === 'int' ? String(value.value) : 'None'
        }}
        onParseCellText={async (_coordinate, _path, text) => ({ kind: 'string', value: JSON.parse(text) as string })}
        onWriteFieldBatch={async writes => {
          control.batches++
          await new Promise(resolve => setTimeout(resolve, control.delay))
          if (control.fail) throw new Error('测试批量写入失败')
          setData(current => ({ ...current, revision: current.revision + 1,
            records: current.records.map(record => ({ ...record, fields: record.fields.map(field => {
              const write = writes.find(write => write.coordinate.id === record.coordinate.id && write.field_path[0].value === field.name)
              return write ? { ...field, value: write.new_value } : field
            }) })),
          }))
        }}
        onWriteField={async (coordinate, path, value) => {
          control.writes++
          control.writtenKinds.push(value.kind)
          await new Promise(resolve => setTimeout(resolve, control.delay))
          if (control.fail) throw new Error('测试写入失败')
          setData(current => ({
            ...current, revision: current.revision + 1, columns: structuredClone(current.columns),
            records: current.records.map(row => row.coordinate.id !== coordinate.id ? row : {
              ...row,
              fields: row.fields.map(field => field.name === path[0]?.value ? { ...field, value } : field),
              field_summaries: { ...row.field_summaries, [path[0].value]: plainFieldValueText(value) },
            }),
          }))
        }} />
    </div>
    <button id="outside">outside</button>
    <div id="flags"><EnumDirectSelect value={flags} enumType="Permissions" isFlag onCommit={next => setFlags(next as typeof flags)} /><output>{flags.kind === 'enum' ? String(flags.value.value) : 'null'}</output></div>
    <div id="dimension" style={{ width: 300 }}><DimensionCellEditor row={dimension} variant="en"
      onWrite={async (_row, variant, _expected, next) => {
        await new Promise(resolve => setTimeout(resolve, 30))
        setDimension(row => ({ ...row, values: { ...row.values, [variant]: next } }))
      }} /></div>
    <output id="selection">{selection ? `${selection.kind}:${selection.coordinate.id}` : 'none'}</output>
  </>
}

createRoot(document.getElementById('root')!).render(
  <ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={() => {}}><Fixture /></ObjectDraftHost>,
)
