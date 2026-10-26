import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { createMockLookups } from '../../src/mockLookups'
import { EditorLookupController } from '../../src/state/editorLookups'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TableView } from '../../src/components/TableView'
import { InspectorPanel } from '../../src/components/InspectorPanel'
import { MOCK_FILE_RECORDS } from '../../src/mock'
import { updateRecordSelection, updateValueSelection, type EditorSelection, type SelectedCells } from '../../src/state/editorSelection'
import { columnContentMetrics } from '../../src/components/TableView.support'
import type { FieldValue } from '../../src/wire'
import '../../src/style.css'
const params = new URLSearchParams(location.search)
const rows = Number(params.get('rows') ?? 1000), cols = Number(params.get('cols') ?? 6)
const original = MOCK_FILE_RECORDS['data/item.cfd']
const seed = original.records[0]
const fields = seed.fields.slice()
for (let i = fields.length; i < cols; i++) fields.push({ ...fields.find(f => f.value.kind === 'string')!, name: `extra_${i}`, value: { kind: 'string', value: '用于测量单元格显示宽度的内容 '.repeat(8) } })
// 专项夹具控制复杂列数量、集合长度与嵌套深度，不改变默认标量基准。
const complexCols = Number(params.get('complexCols') ?? 0)
const items = Number(params.get('items') ?? 8)
const depth = Number(params.get('depth') ?? 1)
function complexValue(level: number): FieldValue {
  if (level === 0) return { kind: 'string', value: '复杂预览文本，用于采样布局与渲染开销' }
  if (params.get('shape') === 'object') return { kind: 'object', value: {
    actual_type: 'PerformanceObject', fields: Object.fromEntries(Array.from({ length: items }, (_, index) => [`Field${index}`, complexValue(level - 1)])),
  } }
  return { kind: 'array', value: Array.from({ length: items }, (_, index) => ({
    kind: 'object', value: { actual_type: 'PerformanceItem', fields: {
      Label: { kind: 'string', value: `项目 ${index}` },
      Count: { kind: 'int', value: BigInt(index) },
      Nested: complexValue(level - 1),
    } },
  })) }
}
for (let i = fields.length - complexCols; i < fields.length; i++) {
  if (i >= 0) fields[i] = { ...fields[i], value: complexValue(depth) }
}
const records = Array.from({ length: rows }, (_, index) => ({ ...seed,
  coordinate: { ...seed.coordinate, id: `row_${index}` }, container_index: index, container_size: rows,
  fields: fields.map(field => ({ ...field, value: field.name === 'name' ? { kind: 'string' as const, value: `文本 ${index}` } : field.value })),
  field_index: Object.fromEntries(fields.map((field, index) => [field.name, index])),
  field_summaries: Object.fromEntries(fields.map(field => [field.name, field.value.kind === 'string' ? field.value.value : seed.field_summaries[field.name] ?? ''])),
}))
const fixture = { ...original, records, columns: fields.map(field => ({ name: field.name, type_names: ['Item'], max_summary_len: 0,
  content_metrics: columnContentMetrics(records, field.name),
})) }
function Fixture() {
  const [data, setData] = useState(fixture)
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [cells, setCells] = useState<SelectedCells>([])
  return <div style={{ display: 'flex', height: 480 }}><div style={{ width: 880, height: 480 }}>
    <TableView data={data} activeType="Item" selection={selection} onValueSelectionCellsChange={setCells}
      onSelectValue={(coordinate, path, mode = 'replace') => setSelection(current => updateValueSelection(current, data.file_path, coordinate, path, mode))}
      onSelectRecord={(coordinate, mode, visible) => setSelection(current => updateRecordSelection(current, data.file_path, coordinate, visible, mode))}
      onClearSelection={() => setSelection(null)} onOpenRecord={() => {}}
      onWriteField={async (coordinate, path, value) => {
        setData(current => ({ ...current, records: current.records.map(row => row.coordinate.id === coordinate.id ? { ...row, fields: row.fields.map(field => field.name === path[0].value ? { ...field, value } : field) } : row) }))
      }} />
    </div>{params.has('inspector') && <InspectorPanel open embedded data={data} selection={selection} valueCells={cells} width={320} onWidthChange={() => {}} onClose={() => {}} />}</div>
}
const lookups = new EditorLookupController(createMockLookups(() => MOCK_FILE_RECORDS))
lookups.adopt({ sessionId: 1, revision: 1 })
createRoot(document.getElementById('root')!).render(<ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={() => {}}><Fixture /></ObjectDraftHost>)
