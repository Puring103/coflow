import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TableView } from '../../src/components/TableView'
import { DimensionCellEditor } from '../../src/components/DimensionTableView'
import { MOCK_FILE_RECORDS } from '../../src/mock'
import { createMockLookups } from '../../src/mockLookups'
import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { EditorLookupController } from '../../src/state/editorLookups'
import type { DimensionFileRow } from '../../src/api'
import { updateRecordSelection, updateValueSelection, type EditorSelection } from '../../src/state/editorSelection'
import { plainFieldValueText } from '../../src/value/fieldValue'
import '../../src/style.css'

// 使用真实表格和异步快照更新，覆盖保存期间的 DOM 身份与焦点行为。
const original = MOCK_FILE_RECORDS['data/item.cfd']
const seed = original.records[0]
const fixture = {
  ...original,
  records: Array.from({ length: 100 }, (_, index) => ({
    ...structuredClone(seed),
    coordinate: { ...seed.coordinate, id: `row_${index}` },
    container_index: index,
    fields: seed.fields.map(field => field.name === 'name' ? {
      ...field, value: { kind: 'string' as const, value: index === 2 ? '这是一段用于验证长文本展开的字符串。'.repeat(14) : `文本 ${index}` },
    } : structuredClone(field)),
  })),
}

const control = { writes: 0, fail: false, delay: 40, selectionEvents: [] as string[] }
Object.assign(window, { tableTest: control })
const lookups = new EditorLookupController(createMockLookups(() => MOCK_FILE_RECORDS))
lookups.adopt({ sessionId: 1, revision: 1 })

function Fixture() {
  const [data, setData] = useState(fixture)
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [dimension, setDimension] = useState<DimensionFileRow>({
    coordinate: seed.coordinate, field: 'name', owner_file_path: data.file_path,
    default_value: { kind: 'string', value: '基础文本' }, values: {}, sources: {},
  })
  return <>
    <div style={{ height: 480, width: 880 }}>
      <TableView data={data} activeType="Item" selection={selection}
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
        onWriteField={async (coordinate, path, value) => {
          control.writes++
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
