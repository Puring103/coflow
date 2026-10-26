import { GitDiffMode, type GitDiffSelection } from '../../src/features/git-diff/components/GitDiffMode'
import { DimensionTableView } from '../../src/features/table/components/DimensionTableView'
import type { DimensionFileRecords } from '../../src/bindings/DimensionFileRecords'
import type { ProjectDiff } from '../../src/bindings/ProjectDiff'
import type { FieldValue } from '../../src/wire'
import { StrictMode, useEffect, useState, useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { InspectorPanel } from '../../src/features/record-editing/components/InspectorPanel'
import { ReferencePanel } from '../../src/features/record-editing/components/ReferencePanel'
import { TableView } from '../../src/features/table/components/TableView'
import { ObjectDraftHost } from '../../src/features/record-editing/components/ObjectDraftHost'
import { EditorLookupController } from '../../src/features/workspace/state/editorLookups'
import { registerDataEdit } from '../../src/features/record-editing/state/editSession'
import { recordSelection, valueSelection, type EditorSelection } from '../../src/features/workspace/state/editorSelection'
import { cellMotionOrigin, originBounds, revealAndEmphasize, type MotionOrigin } from '../../src/shared/motion/motion'
import { MOCK_FILE_RECORDS } from '../../src/test-support/mock'
import { createMockLookups } from '../../src/test-support/mockLookups'
import { committed, MutationHistoryController } from '../../src/features/workspace/state/editorState'
import { useMutationFeedback } from '../../src/features/record-editing/hooks/useMutationFeedback'
import type { FileRecords } from '../../src/bindings/FileRecords'
import '../../src/style.css'

const complex = new URLSearchParams(location.search).has('complex')
const original = structuredClone(MOCK_FILE_RECORDS['data/item.cfd'])
if (new URLSearchParams(location.search).has('overflow')) {
  original.records[0].fields.find(field => field.name === 'name')!.value = { kind: 'string', value: '长文本预览 '.repeat(200) }
}
const data: FileRecords = complex ? {
  ...original,
  columns: [...original.columns, { ...original.columns[0], name: 'Config' }],
  records: original.records.map(row => ({ ...row,
    field_index: { ...row.field_index, Config: row.fields.length },
    fields: [...row.fields, { name: 'Config', missing: false, annotation: null, value: {
      kind: 'object', value: { actual_type: 'Config', fields: { Target: { kind: 'ref', value: 'sample' }, Amount: { kind: 'float', value: 0.25 }, References: { kind: 'array', value: [{ kind: 'ref', value: 'sample' }] }, Text: { kind: 'string', value: '对象内部文本' } } },
    } }],
  })),
} : original
const lookups = new EditorLookupController(createMockLookups(() => MOCK_FILE_RECORDS))
lookups.adopt({ sessionId: 1, revision: data.revision })
const control = { fail: false, delay: 0, commits: 0, closes: 0 }

function Fixture() {
  const [file, setFile] = useState(data)
  const [history] = useState(() => new MutationHistoryController())
  const snapshot = useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot)
  const [open, setOpen] = useState(false)
  const [origin, setOrigin] = useState<MotionOrigin | null>(null)
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [references, setReferences] = useState<string[]>([])
  useMutationFeedback(snapshot, origin, open)
  useEffect(() => registerDataEdit(async () => {
    control.commits++
    await new Promise(resolve => setTimeout(resolve, control.delay))
    return !control.fail
  }, () => false), [])
  Object.assign(window, { motionTest: {
    control,
    commit: () => history.record({ kind: 'field', revision: snapshot.undo.length + 1, filePath: data.file_path,
      coordinate: data.records[0].coordinate, fieldPath: [{ kind: 'field', value: 'name' }],
      oldValue: { kind: 'string', value: '旧值' }, newValue: { kind: 'string', value: '新值' } }),
    undo: () => history.undo(async () => committed(undefined)),
    failedUndo: () => history.undo(async () => ({ status: 'failed' })),
    bounds: () => originBounds(origin),
    reference: () => setReferences(['A']),
    open: () => {
      const cell = document.querySelector<HTMLElement>('td[data-field="name"]')!
      setSelection(valueSelection(data.file_path, data.records[0].coordinate, [{ kind: 'field', value: 'name' }]))
      setOrigin(cellMotionOrigin(cell))
      setOpen(true)
    },
    record: () => { setSelection(recordSelection(data.file_path, data.records[0].coordinate)); setOrigin(null); setOpen(true) },
    generic: () => { setOrigin(null); setOpen(true) },
    remove: () => document.querySelector('td[data-field="name"]')?.remove(),
    reveal: (index: number) => revealAndEmphasize(document.getElementById(`target-${index}`)!, document.getElementById('scroller')!),
  } })
  return <ObjectDraftHost lookups={lookups} generationKey="1" sessionId={1} onOpenReference={() => {}}>
    <div className="app" style={{ display: 'block', padding: 24 }}>
      <div style={{ height: 380, width: 850 }}>
        <TableView onWriteField={new URLSearchParams(location.search).has('overflow') ? async () => {} : undefined} data={file} activeType={data.records[0].coordinate.actual_type} selection={selection}
          visibleColumns={complex ? ['name', 'Config'] : ['name']} onOpenRecord={() => {}}
          onSelectRecord={coordinate => setSelection(recordSelection(file.file_path, coordinate))}
          onClearSelection={() => { setSelection(null); setOpen(false) }}
          onCreateRecordDraft={async actual_type => ({ actual_type, fields: [] })}
          onMoveRecord={async () => {}}
          onInsertRecord={async (id, actual_type) => {
            setFile(current => ({ ...current, revision: current.revision + 1, records: [...current.records, {
              ...current.records[0], coordinate: { id, actual_type }, container_index: current.records.length,
            }] }))
          }}
          onDeleteRecords={async coordinates => {
            setFile(current => ({ ...current, revision: current.revision + 1,
              records: current.records.filter(record => !coordinates.some(coordinate => coordinate.id === record.coordinate.id)) }))
          }}
          onSelectValue={(coordinate, path) => setSelection(valueSelection(data.file_path, coordinate, path))}
          onExpandCell={(coordinate, path, source) => {
            setSelection(valueSelection(data.file_path, coordinate, path ?? [{ kind: 'field', value: 'name' }]))
            setOrigin(source); setOpen(true)
          }} />
      </div>
      <div id="scroller" style={{ height: 160, overflowY: 'auto', width: 300 }}>
        {Array.from({ length: 30 }, (_, index) => <div id={`target-${index}`} key={index} style={{ height: 40 }}>目标 {index}</div>)}
      </div>
    </div>
    <InspectorPanel modal open={open} origin={origin} data={file} selection={selection} width={420} onWidthChange={() => {}}
      onClose={() => { control.closes++; setOpen(false) }} />
    {references.length > 0 && <StrictMode><ReferencePanel title={references[references.length - 1]} recordId={references[references.length - 1]}
      actualType="Item" depth={references.length} onClose={() => setReferences([])}
      onBack={() => setReferences(current => current.slice(0, -1))} onNavigate={() => setReferences([])}>
      <button onClick={() => setReferences(current => [...current, 'B'])}>打开下一引用</button>
    </ReferencePanel></StrictMode>}
  </ObjectDraftHost>
}

const extensionValue: FieldValue = { kind: 'array', value: Array.from({ length: 24 }, (_, index) => ({ kind: 'string', value: `value-${index}` })) }
const extensionDiff: ProjectDiff = {
  head_oid: '1234567890', target_revision: 1, semantic_available: true, diagnostics: [],
  files: [{ path: 'items.cfd', change: 'modified', before: 'match before', after: 'match after match', patch: '' }],
  records: [{ coordinate: { actual_type: 'Item', id: 'sample' }, change: 'modified',
    before: { file_path: 'items.cfd', values: [{ path: 'Items', value: extensionValue }] },
    after: { file_path: 'items.cfd', values: [{ path: 'Items', value: extensionValue }] },
    fields: [{ path: 'Items[0]', change: 'modified' }] }],
}
function ExtensionFixture() {
  const [selection, setSelection] = useState<GitDiffSelection>({ filePath: 'items.cfd', typeName: 'Item', coordinate: null })
  const [mode, setMode] = useState<'table' | 'record'>('table')
  const [dimension, setDimension] = useState<DimensionFileRecords>({ revision: 1, file_path: 'language.cfd', dimension: 'language', display_name: 'Language', variants: ['en'],
    rows: ['first', 'second'].map(id => ({ coordinate: { actual_type: 'Item', id }, owner_file_path: 'items.cfd', field: 'Items', default_value: extensionValue, values: {}, sources: {} })) })
  return <ObjectDraftHost lookups={lookups} generationKey="1" sessionId={1} onOpenReference={() => {}}>
    <div className="app" style={{ height: '100vh', padding: 20 }}>
      {new URLSearchParams(location.search).get('extension') === 'diff'
        ? <GitDiffMode diff={extensionDiff} sessionId={1} loading={false} error={null} selection={selection} onSelectionChange={setSelection} onRefresh={() => {}} fileTypes={{ 'items.cfd': [{ name: 'Item', display_name: 'Item', record_count: 1, is_singleton: false, dimension_fields: {} }] }} />
        : <DimensionTableView data={dimension} mode={mode} onModeChange={setMode} onWrite={async (row, variant, _expected, next) => {
          setDimension(current => ({ ...current, revision: current.revision + 1, rows: current.rows.map(item => item.coordinate.id === row.coordinate.id ? { ...item, values: { ...item.values, [variant]: next } } : item) }))
        }} />}
    </div>
  </ObjectDraftHost>
}
createRoot(document.getElementById('root')!).render(new URLSearchParams(location.search).has('extension') ? <ExtensionFixture /> : <Fixture />)
