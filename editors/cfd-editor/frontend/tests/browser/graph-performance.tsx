import { useEffect, useRef, useState } from 'react'
import { ReactFlowProvider, useNodes, useReactFlow } from '@xyflow/react'
import { createRoot } from 'react-dom/client'
import type { FieldAnnotation } from '../../src/bindings/FieldAnnotation'
import type { FieldCell } from '../../src/bindings/FieldCell'
import type { GraphData } from '../../src/bindings/GraphData'
import type { CollectionEdit } from '../../src/bindings/CollectionEdit'
import { GraphView } from '../../src/components/GraphView'
import { DataCardNode } from '../../src/components/DataCard'
import type { ComponentProps } from 'react'
import type { GraphRowGeometry } from '../../src/graph/rowGeometry'
import { ObjectDraftHost } from '../../src/components/ObjectDraftHost'
import { ShortNameContext } from '../../src/components/ShortNameContext'
import { EditorLookupController } from '../../src/state/editorLookups'
import { createMockLookups } from '../../src/mockLookups'
import { coordinateId, dictKeyPathText, type FieldPathSegment, type FieldValue } from '../../src/wire'
import '../../src/style.css'

const params = new URLSearchParams(location.search)
const count = Number(params.get('items') ?? 100)
const editable = params.has('editable')
const kind = params.get('kind') ?? 'refs'
const referenceCount = kind === 'refs' ? count : 8
const annotation = (type: string, ref: string | null = null): FieldAnnotation => ({
  declared_type: type, ref_target_type: ref, enum_int_value: null, enum_type: null,
  enum_is_flag: false, nullable: false, read_only: false, item_annotation: null,
  polymorphic_types: [], object_type: null, field_order: [], children: {},
})
const coordinate = (id: string) => ({ actual_type: 'Item', id })
const refs: FieldCell = {
  name: 'links', missing: false,
  value: { kind: 'array', value: Array.from({ length: referenceCount }, () => ({ kind: 'ref', value: 'target' })) },
  annotation: { ...annotation('[&Item]'), item_annotation: annotation('&Item', 'Item') },
}
const objectAnnotation: FieldAnnotation = { ...annotation('Option<Details>'), object_type: 'Details', polymorphic_types: ['Details', 'OtherDetails'],
  field_order: ['title', 'number'], children: { title: annotation('string'), number: annotation('int') } }
const extra: FieldCell[] = kind === 'refs' ? [] : [{ name: 'values', missing: false,
  annotation: kind === 'dict' ? { ...annotation('{string: string}'), item_annotation: annotation('string') }
    : { ...annotation(kind === 'objects' ? '[Option<Details>]' : kind === 'strings' ? '[string]' : '[int]'),
      item_annotation: kind === 'objects' ? objectAnnotation : annotation(kind === 'strings' ? 'string' : 'int') },
  value: kind === 'dict' ? { kind: 'dict', value: Array.from({ length: count }, (_, index) => [{ kind: 'string', value: `key${index}.x["z"]` }, { kind: 'string', value: `文字 ${index}` }]) }
    : { kind: 'array', value: Array.from({ length: count }, (_, index): FieldValue => kind === 'objects'
      ? { kind: 'option_some', value: { kind: 'object', value: { actual_type: 'Details', fields: { title: { kind: 'string', value: `标题 ${index}` }, number: { kind: 'int', value: BigInt(index) } } } } }
      : kind === 'strings' ? { kind: 'string', value: `文字 ${index}\n第二行文字 ${index}` } : { kind: 'int', value: BigInt(index) }) },
}]
const graph: GraphData = {
  revision: 1, available_fields: ['links'],
  nodes: ['source', 'target', 'other'].map(id => ({
    coordinate: coordinate(id), file_path: 'graph.cfd', in_focus_file: true, is_collapsed: false,
    diagnostic_severity: null, field_diagnostics: [], fields: [
      { name: 'name', missing: false, value: { kind: 'string', value: id }, annotation: annotation('string') },
      ...(id === 'source' ? [...extra, refs] : []),
    ],
  })),
  edges: Array.from({ length: referenceCount }, (_, index) => ({
    source: coordinate('source'), target: coordinate('target'), field_path: `links[${index}]`,
  })),
}
const backend = createMockLookups(() => ({}))
backend.getRefTargets = async () => graph.nodes.map(node => ({ coordinate: node.coordinate, file_path: node.file_path, short_name: node.coordinate.id }))
const lookups = new EditorLookupController(backend)
lookups.adopt({ sessionId: 1, revision: 1 })
const positions = Object.fromEntries(graph.nodes.map((node, index) => [coordinateId(node.coordinate),
  params.has('standard') && index > 0 ? [100000 + index * 400, count * 34] as [number, number] : [index * 400, 0] as [number, number]]))
const events: unknown[] = []

function FlowControl() {
  const flow = useReactFlow()
  const nodes = useNodes()
  useEffect(() => { Object.assign(window, { graphFlow: flow }) }, [flow])
  useEffect(() => {
    if (!nodes.length || nodes.some(node => (node.data as { geometry: GraphRowGeometry }).geometry.pendingCount > 0)
      || (window as Window & { graphStableAt?: number }).graphStableAt !== undefined) return
    let frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => {
      if (params.has('standard')) void flow.setViewport({ x: 100, y: 100, zoom: 1 })
      frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => Object.assign(window, { graphStableAt: performance.now() })) })
    }) })
    return () => cancelAnimationFrame(frame)
  }, [flow, nodes])
  return null
}

// 小夹具用原始完整卡片逐行校验窗口几何，不参与性能测量。
function FullReference() {
  const nodes = useNodes()
  const node = nodes.find(node => node.id === 'Item::source')
  if (!node) return null
  const data = node.data as unknown as ComponentProps<typeof DataCardNode> & {
    graphNode: { fields: FieldCell[]; actual_type: string }; expanded: boolean; onToggleExpand: () => void
    relation: { expanded: ReadonlySet<string>; appendPaths: ReadonlySet<string> }; geometry: GraphRowGeometry
  }
  return <div className="graph-node full-reference" style={{ position: 'fixed', left: -20000, width: 280, visibility: 'hidden' }}>
    <DataCardNode {...data} fields={data.graphNode.fields} actualType={data.graphNode.actual_type}
      showAll={data.expanded} onToggle={data.onToggleExpand}
      graphRelations={{ expandedPaths: data.relation.expanded, appendPaths: data.relation.appendPaths }} />
  </div>
}

function replaceAt(value: FieldValue, path: FieldPathSegment[], change: (value: FieldValue) => FieldValue): FieldValue {
  if (!path.length) return change(value)
  if (value.kind === 'option_some') return { ...value, value: replaceAt(value.value, path, change) }
  const [head, ...tail] = path
  if (head.kind === 'index' && value.kind === 'array') return { ...value, value: value.value.map((item, index) => index === head.value ? replaceAt(item, tail, change) : item) }
  if (head.kind === 'dict_key' && value.kind === 'dict') return { ...value, value: value.value.map(([key, item]) => [key, dictKeyPathText(key) === head.value ? replaceAt(item, tail, change) : item]) }
  if (head.kind === 'field' && value.kind === 'object') return { ...value, value: { ...value.value, fields: { ...value.value.fields, [head.value]: replaceAt(value.value.fields[head.value], tail, change) } } }
  throw new Error('invalid fixture field path')
}
function Example() {
  const [data, setData] = useState(graph)
  const [compact, setCompact] = useState(params.has('compact'))
  const [graphVisible, setGraphVisible] = useState(true)
  const failWrites = useRef(false)
  const update = (record: ReturnType<typeof coordinate>, path: FieldPathSegment[], change: (value: FieldValue) => FieldValue) => {
    setData(previous => {
      const nodes = previous.nodes.map(node => node.coordinate.id !== record.id ? node : { ...node, fields: node.fields.map(field => path[0]?.kind === 'field' && path[0].value === field.name
        ? { ...field, value: replaceAt(field.value, path.slice(1), change) } : field) })
      const source = nodes.find(node => node.coordinate.id === 'source')!.fields.find(field => field.name === 'links')!.value
      const edges = source.kind === 'array' ? source.value.flatMap((value, index) => value.kind === 'ref' ? [{ source: coordinate('source'), target: coordinate(value.value), field_path: `links[${index}]` }] : []) : []
      return { ...previous, nodes, edges, revision: previous.revision + 1 }
    })
  }
  const write = async (_file: string, record: ReturnType<typeof coordinate>, path: FieldPathSegment[], value: FieldValue) => {
    events.push({ kind: 'write', record, path, value })
    if (failWrites.current) throw new Error('fixture write rejected')
    update(record, path, () => value)
  }
  const collection = async (_file: string, record: ReturnType<typeof coordinate>, path: FieldPathSegment[], operation: CollectionEdit) => {
    events.push({ kind: 'collection', record, path, operation })
    update(record, path, value => {
      if (value.kind === 'array') {
        const items = value.value.slice()
        if (operation.kind === 'array_append') items.push(operation.value ?? { kind: 'int', value: 0n })
        if (operation.kind === 'array_remove') items.splice(operation.index, 1)
        if (operation.kind === 'array_move') items.splice(operation.to, 0, items.splice(operation.from, 1)[0])
        return { ...value, value: items }
      }
      if (value.kind === 'dict' && operation.kind === 'dict_remove') return { ...value, value: value.value.filter(([key]) => dictKeyPathText(key) !== dictKeyPathText(operation.key)) }
      throw new Error('unsupported fixture collection operation')
    })
  }
  Object.assign(window, { graphHarness: {
    events, setCompact, setGraphVisible, getData: () => data, setFailWrites: (fail: boolean) => { failWrites.current = fail },
    // 输入由测试驱动，组件内部仍使用正常的不可变快照更新。
    setData,
  } })
  return <ReactFlowProvider><FlowControl /><ObjectDraftHost lookups={lookups} generationKey={`1:${data.revision}`} sessionId={1} onOpenReference={() => {}}>
    <ShortNameContext.Provider value={{ fields: {} }}>
      <div style={{ width: '100vw', height: '100vh' }}>
        {params.has('reference') && <FullReference />}
        {graphVisible && <GraphView viewKey="graph-performance" graphData={data} filePath="graph.cfd" activeType="Item"
          savedPositions={positions} savedCompact={compact} onSaveCompact={setCompact}
          onWriteField={editable ? write : undefined}
          onCollectionEdit={editable ? collection : undefined}
          onCreateRecordDraft={params.has('create') ? async actual_type => ({ actual_type, fields: [] }) : undefined}
          onInsertRecord={params.has('create') ? async (id, actual_type, payload) => {
            events.push({ kind: 'insert', id, actual_type })
            setData(previous => ({ ...previous, revision: previous.revision + 1, nodes: [...previous.nodes, {
              coordinate: { actual_type, id }, file_path: 'graph.cfd', in_focus_file: true, is_collapsed: false,
              diagnostic_severity: null, field_diagnostics: [], fields: payload.kind === 'object' ? Object.entries(payload.value.fields).map(([name, value]) => ({ name, value, missing: false, annotation: annotation('string') })) : [],
            }] }))
          } : undefined}
          onOpenRecord={(_file, record) => { events.push({ kind: 'open', record }) }} />}
      </div>
    </ShortNameContext.Provider>
  </ObjectDraftHost></ReactFlowProvider>
}
Object.assign(window, { graphDataReadyAt: performance.now() })
createRoot(document.getElementById('root')!).render(<Example />)
