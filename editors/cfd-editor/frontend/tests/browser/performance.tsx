import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { createMockLookups } from '../../src/mockLookups'
import { EditorLookupController } from '../../src/state/editorLookups'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TableView } from '../../src/components/TableView'
import { InspectorPanel } from '../../src/components/InspectorPanel'
import { MOCK_FILE_RECORDS } from '../../src/mock'
import { updateRecordSelection, updateValueSelection, type EditorSelection, type SelectedCells } from '../../src/state/editorSelection'
import { estimateTextWidth } from '../../src/components/TableView.support'
import '../../src/style.css'
const params = new URLSearchParams(location.search)
const rows = Number(params.get('rows') ?? 1000), cols = Number(params.get('cols') ?? 6)
const original = MOCK_FILE_RECORDS['data/item.cfd']
const seed = original.records[0]
const fields = seed.fields.slice()
for (let i = fields.length; i < cols; i++) fields.push({ ...fields.find(f => f.value.kind === 'string')!, name: `extra_${i}`, value: { kind: 'string', value: '用于测量单元格显示宽度的内容 '.repeat(8) } })
const records = Array.from({ length: rows }, (_, index) => ({ ...seed,
  coordinate: { ...seed.coordinate, id: `row_${index}` }, container_index: index, container_size: rows,
  fields: fields.map(field => ({ ...field, value: field.name === 'name' ? { kind: 'string' as const, value: `文本 ${index}` } : field.value })),
  field_index: Object.fromEntries(fields.map((field, index) => [field.name, index])),
  field_summaries: Object.fromEntries(fields.map(field => [field.name, field.value.kind === 'string' ? field.value.value : seed.field_summaries[field.name] ?? ''])),
}))
const fixture = { ...original, records, columns: fields.map(field => ({ name: field.name, type_names: ['Item'], max_summary_len: 0,
  content_metrics: { Item: { max_width: estimateTextWidth(records[0].field_summaries[field.name] ?? '', false, 600), has_complex_value: ['array','dict','object'].includes(field.value.kind) } },
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
