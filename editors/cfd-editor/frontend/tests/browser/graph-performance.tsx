import { useEffect, useState } from 'react'
import { ReactFlowProvider, useReactFlow } from '@xyflow/react'
import { createRoot } from 'react-dom/client'
import type { FieldAnnotation } from '../../src/bindings/FieldAnnotation'
import type { FieldCell } from '../../src/bindings/FieldCell'
import type { GraphData } from '../../src/bindings/GraphData'
import { GraphView } from '../../src/components/GraphView'
import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { ShortNameContext } from '../../src/components/ShortNameContext'
import { EditorLookupController } from '../../src/state/editorLookups'
import { createMockLookups } from '../../src/mockLookups'
import { coordinateId, type FieldPathSegment, type FieldValue } from '../../src/wire'
import '../../src/style.css'

const params = new URLSearchParams(location.search)
const count = Number(params.get('items') ?? 100)
const editable = params.has('editable')
const annotation = (type: string, ref: string | null = null): FieldAnnotation => ({
  declared_type: type, ref_target_type: ref, enum_int_value: null, enum_type: null,
  enum_is_flag: false, nullable: false, read_only: false, item_annotation: null,
  polymorphic_types: [], object_type: null, field_order: [], children: {},
})
const coordinate = (id: string) => ({ actual_type: 'Item', id })
const refs: FieldCell = {
  name: 'links', missing: false,
  value: { kind: 'array', value: Array.from({ length: count }, () => ({ kind: 'ref', value: 'target' })) },
  annotation: { ...annotation('[&Item]'), item_annotation: annotation('&Item', 'Item') },
}
const graph: GraphData = {
  revision: 1, available_fields: ['links'],
  nodes: ['source', 'target', 'other'].map(id => ({
    coordinate: coordinate(id), file_path: 'graph.cfd', in_focus_file: true, is_collapsed: false,
    diagnostic_severity: null, field_diagnostics: [], fields: [
      { name: 'name', missing: false, value: { kind: 'string', value: id }, annotation: annotation('string') },
      ...(id === 'source' ? [refs] : []),
    ],
  })),
  edges: Array.from({ length: count }, (_, index) => ({
    source: coordinate('source'), target: coordinate('target'), field_path: `links[${index}]`,
  })),
}
const backend = createMockLookups(() => ({}))
backend.getRefTargets = async () => graph.nodes.map(node => ({ coordinate: node.coordinate, file_path: node.file_path, short_name: node.coordinate.id }))
const lookups = new EditorLookupController(backend)
lookups.adopt({ sessionId: 1, revision: 1 })
const positions = Object.fromEntries(graph.nodes.map((node, index) => [coordinateId(node.coordinate), [index * 400, 0] as [number, number]]))
const events: unknown[] = []

function FlowControl() {
  const flow = useReactFlow()
  useEffect(() => { Object.assign(window, { graphFlow: flow }) }, [flow])
  return null
}

function Example() {
  const [data, setData] = useState(graph)
  const [compact, setCompact] = useState(params.has('compact'))
  const write = async (_file: string, record: ReturnType<typeof coordinate>, path: FieldPathSegment[], value: FieldValue) => {
    events.push({ kind: 'write', record, path, value })
    setData(previous => ({ ...previous, revision: previous.revision + 1,
      nodes: previous.nodes.map(node => node.coordinate.id !== record.id ? node : {
        ...node, fields: node.fields.map(field => {
          if (path[0]?.kind !== 'field' || path[0].value !== field.name) return field
          if (path.length === 1) return { ...field, value }
          if (path[1]?.kind !== 'index' || field.value.kind !== 'array') throw new Error('unsupported fixture write')
          const items = field.value.value.slice()
          items[path[1].value] = value
          return { ...field, value: { kind: 'array', value: items } }
        }),
      }),
    }))
  }
  Object.assign(window, { graphHarness: {
    events, setCompact, getData: () => data,
    // 输入由测试驱动，组件内部仍使用正常的不可变快照更新。
    setData,
  } })
  return <ReactFlowProvider><FlowControl /><ObjectDraftHost lookups={lookups} generationKey={`1:${data.revision}`} sessionId={1} onOpenReference={() => {}}>
    <ShortNameContext.Provider value={{ fields: {} }}>
      <div style={{ width: '100vw', height: '100vh' }}>
        <GraphView viewKey="graph-performance" graphData={data} filePath="graph.cfd" activeType="Item"
          savedPositions={positions} savedCompact={compact} onSaveCompact={setCompact}
          onWriteField={editable ? write : undefined}
          onCollectionEdit={editable ? async (_file, record, path, operation) => { events.push({ kind: 'collection', record, path, operation }) } : undefined}
          onOpenRecord={(_file, record) => { events.push({ kind: 'open', record }) }} />
      </div>
    </ShortNameContext.Provider>
  </ObjectDraftHost></ReactFlowProvider>
}
createRoot(document.getElementById('root')!).render(<Example />)
