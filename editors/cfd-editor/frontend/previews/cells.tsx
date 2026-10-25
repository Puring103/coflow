import { useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TableView } from '../src/components/TableView'
import { ObjectDraftHost } from '../src/components/ObjectDraftHost'
import { InspectorPanel } from '../src/components/InspectorPanel'
import type { FileRecords } from '../src/bindings/FileRecords'
import { recordSelection, valueSelection, updateValueSelection, updateRecordSelection, type EditorSelection } from '../src/state/editorSelection'
import { DataCardExpanded } from '../src/components/DataCard'
import { createMockLookups } from '../src/mockLookups'
import { MOCK_FILE_RECORDS } from '../src/mock'
import { EditorLookupController } from '../src/state/editorLookups'
import type { FieldAnnotation } from '../src/bindings/FieldAnnotation'
import type { DictKey, FieldPathSegment, FieldValue } from '../src/wire'
import { coordinateId, dictKeyPathText } from '../src/wire'
import '../src/style.css'
import './cells.css'

type Sample = { id: string; name: string; type: string; hint: string; value: FieldValue; annotation: FieldAnnotation }
const str = (value: string): FieldValue => ({ kind: 'string', value })
const num = (value: number): FieldValue => ({ kind: 'int', value: BigInt(value) })
const obj = (type: string, fields: Record<string, FieldValue>): FieldValue => ({ kind: 'object', value: { actual_type: type, fields } })
const arr = (...value: FieldValue[]): FieldValue => ({ kind: 'array', value })
const dict = (entries: Record<string, FieldValue>): FieldValue => ({ kind: 'dict', value: Object.entries(entries).map(([key, value]) => [{ kind: 'string', value: key }, value]) })
const ann = (declared_type: string, overrides: Partial<FieldAnnotation> = {}): FieldAnnotation => ({
  declared_type, enum_int_value: null, ref_target_type: null, enum_type: null, enum_is_flag: false,
  nullable: false, read_only: false, item_annotation: null, polymorphic_types: [], object_type: null, field_order: [], children: {}, ...overrides,
})
const item = Object.values(MOCK_FILE_RECORDS).flatMap(file => file.records).find(row => row.coordinate.actual_type === 'Item')!
const reference = (): FieldValue => ({ kind: 'ref', value: item.coordinate.id })
const refAnn = ann('&Item', { ref_target_type: 'Item' })
const range = (min: number, max: number) => obj('Range', { Min: num(min), Max: num(max) })
const reward = (weight: number) => obj('Reward', { Item: reference(), Weight: num(weight), Count: range(1, 3) })
const rewardAnn = ann('Reward', { children: { Item: refAnn, Weight: ann('int'), Count: ann('Range') } })
const polymorphicAnnotation = ann('Effect', { object_type: 'Effect', polymorphic_types: ['Damage', 'Heal'] })
const initial: Sample[] = [
  { id: 'basics', name: '简单值，直接编辑', type: 'ScalarConfig', hint: '文字、数值、布尔、枚举和引用共用一套安静的字段布局。点击值即可编辑，& 打开引用。',
    value: obj('ScalarConfig', { Name: str('向日葵'), Water: num(64), Growth: { kind: 'float', value: 0.25 }, Enabled: { kind: 'bool', value: true }, Quality: { kind: 'enum', value: { enum_name: 'Quality', variant: 'Rare', value: 2n } }, Template: reference() }),
    annotation: ann('ScalarConfig', { children: { Name: ann('string', { label: '名称' }), Water: ann('int', { label: '初始水量' }), Growth: ann('float', { label: '生长系数' }), Enabled: ann('bool', { label: '启用' }), Quality: ann('Quality', { label: '品质', enum_type: 'Quality' }), Template: { ...refAnn, label: '引用模板' } } }) },
  { id: 'text', name: '长文本', type: 'string', hint: '预览仅展示完整行；底部渐隐表达未显示的内容。聚焦后展开，Enter 保存，Esc 撤销。',
    value: str('可以把你附在体表或位于宿主体内的寄生物选为普通攻击目标，还进入宿主体内，也不伤害宿主；攻击距离使用 Ability_Attack_Range，单次伤害使用 Ability_AttackValue。\n当宿主进入休眠状态时，寄生物仍然保持当前行为。再次施放会刷新持续时间，并保留原有生长进度。宿主离开水域后会逐渐恢复行动能力，而寄生物的持续时间独立计算。每个生长阶段都有不同的营养需求，满足条件后进入下一阶段；若营养不足则暂停生长，不清除已经积累的进度。适合与其他植物的光照和灌溉能力搭配使用。'), annotation: ann('string') },
  { id: 'plant', name: '对象与嵌套对象', type: 'PlantTemplateConfig', hint: '结构标题负责展开，类型信息退到次级。相同层级的字段和值对齐；编辑不会覆盖标签。',
    value: obj('PlantTemplateConfig', { Core: obj('PlantCoreConfig', { Name: str('向日葵'), GrowthDays: num(7), Perennial: { kind: 'bool', value: true } }), Plant: obj('PlantAbilityConfig', { Attack: num(12), Radius: { kind: 'float', value: 2.5 } }), RootTerrains: arr(str('草地'), str('沙地'), str('湿地')), InitialWater: num(64), InitialOrganicRaw: num(20), InitialNutrition: num(80), InitialEnergy: num(8) }),
    annotation: ann('PlantTemplateConfig') },
  { id: 'list', name: '对象数组', type: '[Reward]', hint: '用内容区分列表项，编号仅辅助定位。每项可展开，字段直接编辑。', value: arr(reward(60), reward(30), reward(10)), annotation: ann('[Reward]', { item_annotation: rewardAnn }) },
  { id: 'dictionary', name: '字典中的对象', type: '{string: Biome}', hint: '字典 key 与对象字段分层显示。展开后保留 key，上下文不会因编辑丢失。',
    value: dict({ Forest: obj('Biome', { Temperature: num(22), Moisture: { kind: 'float', value: 0.8 }, Plants: arr(str('蕨类'), str('苔藓'), str('灌木')) }), Desert: obj('Biome', { Temperature: num(38), Moisture: { kind: 'float', value: 0.1 }, Plants: arr(str('仙人掌')) }) }), annotation: ann('{string: Biome}') },
  { id: 'nested', name: '多层混合嵌套', type: 'WorldConfig', hint: '对象 → 数组 → 对象 → 字典 → 数组 → 对象。逐层展开，仅增强正在操作的那一层。',
    value: obj('WorldConfig', { Name: str('温带世界'), Stages: arr(obj('Stage', { Name: str('春季'), Duration: num(30), Regions: dict({ North: arr(reward(80), reward(20)), South: arr(reward(100)) }) }), obj('Stage', { Name: str('夏季'), Duration: num(45), Regions: dict({ Coast: arr(reward(100)) }) })) }),
    annotation: ann('WorldConfig', { children: { Stages: ann('[Stage]', { item_annotation: ann('Stage', { children: { Regions: ann('{string: [Reward]}', { item_annotation: ann('[Reward]', { item_annotation: rewardAnn }) }) } }) }) } }) },
  { id: 'states', name: '空值与空集合', type: 'OptionalConfig', hint: '未设置、空字符串、空数组、空字典分别表达。可选引用可从当前格直接赋值。',
    value: obj('OptionalConfig', { Target: { kind: 'option_none' }, Note: str(''), Tags: arr(), Overrides: dict({}) }),
    annotation: ann('OptionalConfig', { children: { Target: ann('Option<&Item>', { ref_target_type: 'Item', nullable: true }), Note: ann('string'), Tags: ann('[string]', { item_annotation: ann('string') }), Overrides: ann('{string: int}', { item_annotation: ann('int') }) } }) },
  { id: 'polymorphic', name: '多态字段', type: 'AbilityConfig', hint: '折叠时类型跟随字段名，展开后在右侧显示。可写字段可以切换类型。',
    value: obj('AbilityConfig', { Effect: obj('Damage', { Amount: num(20), Range: num(3) }), Passive: obj('Heal', { Amount: num(8) }) }),
    annotation: ann('AbilityConfig', { children: { Effect: polymorphicAnnotation, Passive: { ...polymorphicAnnotation, read_only: true } } }) },
  { id: 'readonly', name: '可编辑与只读并存', type: 'InheritedConfig', hint: '只读字段仍然可读、可选取；可写字段原位修改。悬停可确认字段的只读状态。',
    value: obj('InheritedConfig', { Source: str('基础配置'), BaseValue: num(100), Multiplier: { kind: 'float', value: 1.2 } }),
    annotation: ann('InheritedConfig', { children: { Source: ann('string', { read_only: true }), BaseValue: ann('int', { read_only: true }), Multiplier: ann('float') } }) },
]
// 示例同样提供声明顺序和逐层注解，模拟真实 schema 传入的字段树。
function completeAnnotation(value: FieldValue, annotation: FieldAnnotation): FieldAnnotation {
  if (value.kind === 'object') return { ...annotation, field_order: Object.keys(value.value.fields), children: Object.fromEntries(Object.entries(value.value.fields).map(([key, child]) => [key, completeAnnotation(child, annotation.children[key] ?? ann(child.kind))])) }
  if (value.kind === 'array') return { ...annotation, children: Object.fromEntries(value.value.map((child, index) => [String(index), completeAnnotation(child, annotation.children[String(index)] ?? annotation.item_annotation ?? ann(child.kind))])) }
  if (value.kind === 'dict') return { ...annotation, children: Object.fromEntries(value.value.map(([key, child]) => [dictKeyPathText(key), completeAnnotation(child, annotation.children[dictKeyPathText(key)] ?? annotation.item_annotation ?? ann(child.kind))])) }
  return annotation
}
initial.forEach(sample => { sample.annotation = completeAnnotation(sample.value, sample.annotation) })
const previewLookups = createMockLookups(() => MOCK_FILE_RECORDS)
const lookups = new EditorLookupController({ ...previewLookups,
  async makeDefaultObject(session, type) {
    if (type === 'Damage') return obj('Damage', { Amount: num(20), Range: num(3) })
    if (type === 'Heal') return obj('Heal', { Amount: num(8) })
    return previewLookups.makeDefaultObject(session, type)
  },
})
lookups.adopt({ sessionId: 1, revision: 1 })

// 示例写入同样沿字段路径定位叶子，不把局部修改替换成整个对象。
function replaceAt(value: FieldValue, path: FieldPathSegment[], next: FieldValue): FieldValue {
  if (!path.length) return next
  if (value.kind === 'option_some') return { ...value, value: replaceAt(value.value, path, next) }
  const [segment, ...rest] = path
  if (value.kind === 'object' && segment.kind === 'field') return { ...value, value: { ...value.value, fields: { ...value.value.fields, [String(segment.value)]: replaceAt(value.value.fields[String(segment.value)], rest, next) } } }
  if (value.kind === 'array' && segment.kind === 'index') return { ...value, value: value.value.map((item, index) => index === Number(segment.value) ? replaceAt(item, rest, next) : item) }
  if (value.kind === 'dict' && segment.kind === 'dict_key') return { ...value, value: value.value.map(([key, item]): [DictKey, FieldValue] => [key, dictKeyPathText(key) === String(segment.value) ? replaceAt(item, rest, next) : item]) }
  throw new Error('示例字段路径与数据不一致')
}
function CellDesignPreview() {
  const [samples, setSamples] = useState(initial)
  const [theme, setTheme] = useState('dark')
  const [writes, setWrites] = useState(0)
  const [selection, setSelection] = useState<EditorSelection | null>(null)
  const [dialog, setDialog] = useState<{ selection: EditorSelection; origin: DOMRect | null } | null>(null)
  const [referenceTarget, setReferenceTarget] = useState<string | null>(null)
  const [heights, setHeights] = useState<Record<string, number>>(() => JSON.parse(localStorage.getItem('coflow-cell-preview-row-heights') ?? '{}'))
  const data = useMemo<FileRecords>(() => ({
    revision: writes + 1, file_path: 'preview.cfd', type_names: ['Preview'],
    capabilities: { can_edit_field: true, can_edit_id: true, can_insert_record: false, can_delete_record: false, can_reorder_records: false, requires_full_refresh_after_write: false },
    columns: ['value', '说明'].map(name => ({ name, type_names: ['Preview'], max_summary_len: 60, content_metrics: { Preview: { max_width: name === 'value' ? 640 : 280, has_complex_value: name === 'value' } } })),
    records: samples.map((sample, index) => ({
      coordinate: { actual_type: 'Preview', id: sample.name }, display_path: 'preview.cfd', container_index: index, container_size: samples.length,
      fields: [{ name: 'value', missing: false, value: sample.value, annotation: sample.annotation }, { name: '说明', missing: false, value: str(sample.hint), annotation: ann('string', { read_only: true }) }],
      field_index: { value: 0, '说明': 1 }, field_summaries: { value: sample.type, '说明': sample.hint }, field_diagnostics: [], diagnostic_severity: null,
    })),
  }), [samples, writes])
  async function write(id: string, path: FieldPathSegment[], next: FieldValue) {
    setSamples(current => current.map(sample => {
      if (sample.name !== id) return sample
      const value = replaceAt(sample.value, path.slice(1), next)
      return { ...sample, value, annotation: completeAnnotation(value, sample.annotation) }
    }))
    setWrites(current => current + 1)
  }
  const open = (coordinate: { actual_type: string; id: string }, path: FieldPathSegment[] | null, origin: DOMRect | null) => {
    const target = path ? valueSelection(data.file_path, coordinate, path) : recordSelection(data.file_path, coordinate)
    setSelection(target)
    setDialog({ selection: target, origin })
  }
  return <ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={(_type, id) => setReferenceTarget(id)}>
    <main className="cell-design-page">
      <header className="cell-design-header"><div><div className="cell-design-eyebrow">COFLOW / DESIGN PREVIEW</div><h1>紧凑浏览，放大编辑</h1><p>9 种真实单元格 · 原位编辑 · 复杂内容按需放大</p></div><button className="btn" onClick={() => { const next = theme === 'dark' ? 'light' : 'dark'; setTheme(next); document.documentElement.dataset.theme = next }}>{theme === 'dark' ? '浅色外观' : '深色外观'}</button></header>
      <div className="cell-design-toolbar"><span>双击 ID 查看整条记录</span><span>复杂单元格：双击空白处或点击左上角放大</span><div><button className="btn" onClick={() => { setHeights({}); localStorage.removeItem('coflow-cell-preview-row-heights') }}>恢复默认行高</button><button className="btn" onClick={() => { setSamples(structuredClone(initial)); setWrites(0) }}>重置示例</button></div></div>
      <div className="cell-design-live-table"><TableView data={data} activeType="Preview" selection={selection}
        columnWidths={{ id: 210, value: 650, '说明': 330 }}
        rowHeights={heights} onRowHeightsChange={next => { setHeights(next); localStorage.setItem('coflow-cell-preview-row-heights', JSON.stringify(next)) }}
        onExpandCell={open} onOpenRecord={coordinate => open(coordinate, null, null)}
        onSelectRecord={(coordinate, mode, visible) => setSelection(current => updateRecordSelection(current, data.file_path, coordinate, visible, mode))}
        onSelectValue={(coordinate, path, mode = 'replace') => setSelection(current => updateValueSelection(current, data.file_path, coordinate, path, mode))}
        onEnterInspector={() => { if (selection) setDialog({ selection, origin: null }) }}
        onWriteField={(coordinate, path, next) => write(coordinate.id, path, next)} />
      </div>
      <footer className="cell-design-footer"><span>拖动行底边调整高度 · 双击底边适配内容 · 右键底边恢复默认。行高在此浏览器独立保存。</span><span role="status">已保存 {writes} 次修改</span></footer>
      <InspectorPanel modal open={!!dialog} origin={dialog?.origin} data={data} selection={dialog?.selection ?? null} width={900} onWidthChange={() => {}} onClose={() => setDialog(null)}
        onWriteField={(_file, coordinate, path, next) => write(coordinate.id, path, next)}
        onRenameRecord={async (_file, coordinate, name) => {
          setSamples(current => current.map(sample => sample.name === coordinate.id ? { ...sample, name } : sample))
          setDialog(current => current ? { ...current, selection: recordSelection(data.file_path, { ...coordinate, id: name }) } : current)
          const old = coordinateId(coordinate), next = coordinateId({ ...coordinate, id: name })
          setHeights(current => { const result = { ...current }; if (old in result) { result[next] = result[old]; delete result[old] }; return result })
        }}/>
      {referenceTarget && <div className="reference-panel-overlay" onClick={() => setReferenceTarget(null)}><section className="reference-panel" onClick={event => event.stopPropagation()}><header className="reference-panel-toolbar"><strong>引用记录 <span className="reference-record-id">{referenceTarget}</span></strong><button className="btn" onClick={() => setReferenceTarget(null)}>关闭</button></header><DataCardExpanded fields={Object.values(MOCK_FILE_RECORDS).flatMap(file => file.records).find(row => row.coordinate.id === referenceTarget)?.fields ?? []}/></section></div>}
    </main>
  </ObjectDraftHost>
}
createRoot(document.getElementById('root')!).render(<CellDesignPreview />)
