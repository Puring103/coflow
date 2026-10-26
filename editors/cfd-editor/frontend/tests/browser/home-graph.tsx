import { useState, type ComponentProps } from 'react'
import { createRoot } from 'react-dom/client'
import { GraphView } from '../../src/features/graph/components/GraphView'
import { ObjectDraftHost } from '../../src/features/record-editing/components/ObjectDraftHost'
import { ShortNameContext } from '../../src/features/record-editing/components/ShortNameContext'
import { EditorLookupController } from '../../src/features/workspace/state/editorLookups'
import { createMockLookups } from '../../src/test-support/mockLookups'
import { coordinateId } from '../../src/wire'
import type { GraphData } from '../../src/bindings/GraphData'
import type { FieldCell } from '../../src/bindings/FieldCell'
import type { FieldAnnotation } from '../../src/bindings/FieldAnnotation'
import skills from './home-skills.json'
import '../../src/style.css'

// 官网截图直接复用编辑器图组件及样式；这里只提供技能树数据，不复制图的实现。
const records = skills as [string, string, string, number, number, number, string[]][]
const annotation = (type: string, ref: string | null = null): FieldAnnotation => ({
  declared_type: type, ref_target_type: ref, enum_int_value: null, enum_type: null,
  enum_is_flag: false, nullable: false, read_only: false, item_annotation: null,
  polymorphic_types: [], object_type: null, field_order: [], children: {},
})
const coordinate = (id: string) => ({ actual_type: 'Skill', id })
const nodes = records.map(([id, name, , tier, power, , prerequisites]) => ({
  coordinate: coordinate(id), file_path: 'skills.cfd', in_focus_file: true, is_collapsed: false,
  diagnostic_severity: null, field_diagnostics: [],
  fields: [
    { name: 'name', value: { kind: 'string', value: name }, missing: false, annotation: annotation('string') },
    { name: 'tier', value: { kind: 'int', value: BigInt(tier) }, missing: false, annotation: annotation('int') },
    { name: 'power', value: { kind: 'int', value: BigInt(power) }, missing: false, annotation: annotation('int') },
    { name: 'prerequisites', value: { kind: 'array', value: prerequisites.map(id => ({ kind: 'ref', value: id })) }, missing: false,
      annotation: { ...annotation('[&Skill]'), item_annotation: annotation('&Skill', 'Skill'),
        children: Object.fromEntries(prerequisites.map((_, index) => [String(index), annotation('&Skill', 'Skill')])) } },
  ] as FieldCell[],
}))
const graph: GraphData = { revision: 1, available_fields: ['prerequisites'], nodes,
  edges: records.flatMap(([id, , , , , , prerequisites]) => prerequisites.map((target, index) => ({
    source: coordinate(id), target: coordinate(target), field_path: `prerequisites[${index}]`,
  }))),
}
const backend = createMockLookups(() => ({}))
backend.getRefTargets = async () => records.map(([id, name]) => ({ coordinate: coordinate(id), file_path: 'skills.cfd', short_name: name }))
const lookups = new EditorLookupController(backend)
lookups.adopt({ sessionId: 1, revision: 1 })
// 保存的布局与实际编辑器格式一致，两条分支在末端汇合。
const positions = Object.fromEntries(records.map(([id], index) => [coordinateId(coordinate(id)),
  [[990, 140], [660, 0], [660, 280], [330, 0], [330, 280], [0, 140]][index] as [number, number],
]))
document.documentElement.dataset.theme = new URLSearchParams(location.search).get('theme') || 'light'
function GraphExample() {
  const [data, setData] = useState(graph)
  const interactive = new URLSearchParams(location.search).has('interactive')
  const remove: NonNullable<ComponentProps<typeof GraphView>['onCollectionEdit']> = async (_file, record, _path, operation) => {
    if (operation.kind !== 'array_remove') throw new Error('测试仅接受删除引用项')
    setData(previous => ({ ...previous, revision: previous.revision + 1,
      nodes: previous.nodes.map(node => node.coordinate.id !== record.id ? node : { ...node, fields: node.fields.map(field => field.name !== 'prerequisites' || field.value.kind !== 'array' ? field : { ...field, value: { kind: 'array', value: field.value.value.filter((_, index) => index !== operation.index) } }) }),
      edges: previous.edges.filter(edge => edge.source.id !== record.id || edge.field_path !== `prerequisites[${operation.index}]`),
    }))
  }
  return (
  <ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={() => {}}>
    <ShortNameContext.Provider value={{ fields: { Skill: 'name' } }}>
      <div style={{ width: 960, height: 500 }}>
        <GraphView viewKey="website-skill-tree" graphData={data} filePath="skills.cfd" activeType="Skill"
          onWriteField={interactive ? async () => {} : undefined} onCollectionEdit={interactive ? remove : undefined}
          savedCompact={false} savedPositions={positions} visibleCardFields={new Set(['name', 'tier', 'power'])} onOpenRecord={() => {}} />
      </div>
    </ShortNameContext.Provider>
  </ObjectDraftHost>
  )
}
createRoot(document.getElementById('root')!).render(<GraphExample />)
