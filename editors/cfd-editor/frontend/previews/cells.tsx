import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { EditableCell } from '../src/components/TableView'
import { ObjectDraftHost } from '../src/components/ObjectDraftHost'
import { DataCardExpanded } from '../src/components/DataCard'
import { createMockLookups } from '../src/mockLookups'
import { MOCK_FILE_RECORDS } from '../src/mock'
import { EditorLookupController } from '../src/state/editorLookups'
import type { FieldAnnotation } from '../src/bindings/FieldAnnotation'
import type { DictKey, FieldPathSegment, FieldValue } from '../src/wire'
import { dictKeyPathText } from '../src/wire'
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
const initial: Sample[] = [
  { id: 'basics', name: '简单值，直接编辑', type: 'ScalarConfig', hint: '文字、数值、布尔、枚举和引用共用一套安静的字段布局。点击值即可编辑，& 打开引用。',
    value: obj('ScalarConfig', { Name: str('向日葵'), Water: num(64), Growth: { kind: 'float', value: 0.25 }, Enabled: { kind: 'bool', value: true }, Quality: { kind: 'enum', value: { enum_name: 'Quality', variant: 'Rare', value: 2n } }, Template: reference() }),
    annotation: ann('ScalarConfig', { children: { Name: ann('string', { label: '名称' }), Water: ann('int', { label: '初始水量' }), Growth: ann('float', { label: '生长系数' }), Enabled: ann('bool', { label: '启用' }), Quality: ann('Quality', { label: '品质', enum_type: 'Quality' }), Template: { ...refAnn, label: '引用模板' } } }) },
  { id: 'text', name: '长文本', type: 'string', hint: '预览仅展示完整行；底部渐隐表达未显示的内容。聚焦后展开，Enter 保存，Esc 撤销。',
    value: str('可以把你附在体表或位于宿主体内的寄生物选为普通攻击目标，还进入宿主体内，也不伤害宿主；攻击距离使用 Ability_Attack_Range，单次伤害使用 Ability_AttackValue。\n当宿主进入休眠状态时，寄生物仍然保持当前行为。再次施放会刷新持续时间，并保留原有生长进度。宿主离开水域后会逐渐恢复行动能力，而寄生物的持续时间独立计算。每个生长阶段都有不同的营养需求，满足条件后进入下一阶段；若营养不足则暂停生长，不清除已经积累的进度。适合与其他植物的光照和灌溉能力搭配使用。'), annotation: ann('string') },
  { id: 'plant', name: '对象与嵌套对象', type: 'PlantTemplateConfig', hint: '结构标题负责展开，类型信息退到次级。相同层级的字段和值对齐；编辑不会覆盖标签。',
    value: obj('PlantTemplateConfig', { Core: obj('PlantCoreConfig', { Name: str('向日葵'), GrowthDays: num(7), Perennial: { kind: 'bool', value: true } }), Plant: obj('PlantAbilityConfig', { Attack: num(12), Radius: { kind: 'float', value: 2.5 } }), RootTerrains: arr(str('草地'), str('沙地'), str('湿地')), InitialWater: num(64), InitialOrganicRaw: num(20), InitialNutrition: num(80), InitialEnergy: num(8) }),
    annotation: ann('PlantTemplateConfig') },
  { id: 'list', name: '对象数组', type: '[Reward]', hint: '序号保持稳定。每一项可展开，引用与嵌套范围都可以直接编辑。', value: arr(reward(60), reward(30), reward(10)), annotation: ann('[Reward]', { item_annotation: rewardAnn }) },
  { id: 'dictionary', name: '字典中的对象', type: '{string: Biome}', hint: '字典 key 与对象字段分层显示。展开后保留 key，上下文不会因编辑丢失。',
    value: dict({ Forest: obj('Biome', { Temperature: num(22), Moisture: { kind: 'float', value: 0.8 }, Plants: arr(str('蕨类'), str('苔藓'), str('灌木')) }), Desert: obj('Biome', { Temperature: num(38), Moisture: { kind: 'float', value: 0.1 }, Plants: arr(str('仙人掌')) }) }), annotation: ann('{string: Biome}') },
  { id: 'nested', name: '多层混合嵌套', type: 'WorldConfig', hint: '对象 → 数组 → 对象 → 字典 → 数组 → 对象。逐层展开，仅增强正在操作的那一层。',
    value: obj('WorldConfig', { Name: str('温带世界'), Stages: arr(obj('Stage', { Name: str('春季'), Duration: num(30), Regions: dict({ North: arr(reward(80), reward(20)), South: arr(reward(100)) }) }), obj('Stage', { Name: str('夏季'), Duration: num(45), Regions: dict({ Coast: arr(reward(100)) }) })) }),
    annotation: ann('WorldConfig', { children: { Stages: ann('[Stage]', { item_annotation: ann('Stage', { children: { Regions: ann('{string: [Reward]}', { item_annotation: ann('[Reward]', { item_annotation: rewardAnn }) }) } }) }) } }) },
  { id: 'states', name: '空值与空集合', type: 'OptionalConfig', hint: '未设置、空字符串、空数组、空字典分别表达。可选引用可从当前格直接赋值。',
    value: obj('OptionalConfig', { Target: { kind: 'option_none' }, Note: str(''), Tags: arr(), Overrides: dict({}) }),
    annotation: ann('OptionalConfig', { children: { Target: ann('Option<&Item>', { ref_target_type: 'Item', nullable: true }), Note: ann('string'), Tags: ann('[string]', { item_annotation: ann('string') }), Overrides: ann('{string: int}', { item_annotation: ann('int') }) } }) },
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
const lookups = new EditorLookupController(createMockLookups(() => MOCK_FILE_RECORDS))
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
function pathsFor(value: FieldValue, prefix = 'value'): string[] {
  if (value.kind === 'object') return Object.entries(value.value.fields).flatMap(([key, item]) => [`${prefix}.${key}`, ...pathsFor(item, `${prefix}.${key}`)])
  if (value.kind === 'array') return value.value.flatMap((item, index) => [`${prefix}[${index}]`, ...pathsFor(item, `${prefix}[${index}]`)])
  if (value.kind === 'dict') return value.value.flatMap(([key, item]) => [`${prefix}[${dictKeyPathText(key)}]`, ...pathsFor(item, `${prefix}[${dictKeyPathText(key)}]`)])
  return []
}
function CellDesignPreview() {
  const [samples, setSamples] = useState(initial)
  const [expanded, setExpanded] = useState<Record<string, ReadonlySet<string>>>({ plant: new Set(['value.Core']), list: new Set(['value[0]', 'value[0].Count']), dictionary: new Set(['value["Forest"]']), nested: new Set(['value.Stages', 'value.Stages[0]', 'value.Stages[0].Regions', 'value.Stages[0].Regions["North"]', 'value.Stages[0].Regions["North"][0]']) })
  const [theme, setTheme] = useState('dark')
  const [writes, setWrites] = useState(0)
  const [referenceTarget, setReferenceTarget] = useState<string | null>(null)
  function write(id: string, path: FieldPathSegment[], next: FieldValue) {
    setSamples(current => current.map(sample => sample.id === id ? { ...sample, value: replaceAt(sample.value, path.slice(1), next) } : sample))
    setWrites(current => current + 1)
  }
  return <ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={(_type, id) => setReferenceTarget(id)}>
    <main className="cell-design-page">
      <header className="cell-design-header"><div><div className="cell-design-eyebrow">COFLOW / DESIGN PREVIEW</div><h1>让数据清晰，让编辑安静</h1><p>同一套单元格，从简单值到多层嵌套。所有可写值均可原位编辑。</p></div><button className="btn" onClick={() => { const next = theme === 'dark' ? 'light' : 'dark'; setTheme(next); document.documentElement.dataset.theme = next }}>{theme === 'dark' ? '浅色外观' : '深色外观'}</button></header>
      <div className="cell-design-toolbar"><span>08 种数据场景</span><span>标签始终保留 · 逐层展开 · 局部编辑</span><div><button className="btn" onClick={() => setExpanded(Object.fromEntries(samples.map(sample => [sample.id, new Set(pathsFor(sample.value))])))}>展开全部</button><button className="btn" onClick={() => setExpanded({})}>收起全部</button><button className="btn" onClick={() => { setSamples(structuredClone(initial)); setWrites(0) }}>重置示例</button></div></div>
      <table className="data-table cell-design-table"><colgroup><col className="scenario-column"/><col className="value-column"/><col className="guide-column"/></colgroup><thead><tr><th>数据场景</th><th>单元格 / 可直接操作</th><th>阅读与操作</th></tr></thead><tbody>{samples.map((sample, index) => <tr key={sample.id} data-sample={sample.id}><td className="cell-design-scenario"><span className="cell-design-index">{String(index + 1).padStart(2, '0')}</span><strong>{sample.name}</strong><code>{sample.type}</code></td><td className={['object', 'array', 'dict'].includes(sample.value.kind) ? 'complex-cell' : ''}><EditableCell filePath="preview.cfd" coordinate={{ actual_type: 'Preview', id: sample.id }} fieldPath={[{ kind: 'field', value: 'value' }]} value={sample.value} annotation={sample.annotation} editable onCommit={next => write(sample.id, [{ kind: 'field', value: 'value' }], next)} onEdit={(path, next) => write(sample.id, path, next)} expandedPaths={expanded[sample.id] ?? new Set()} onRowToggle={(path, open) => setExpanded(current => { const next = new Set(current[sample.id]); if (open) next.add(path); else next.delete(path); return { ...current, [sample.id]: next } })}/></td><td className="cell-design-guide">{sample.hint}</td></tr>)}</tbody></table>
      <footer className="cell-design-footer"><span>修改仅保留在当前预览页面</span><span role="status">已保存 {writes} 次修改</span></footer>
      {referenceTarget && <div className="reference-panel-overlay" onClick={() => setReferenceTarget(null)}><section className="reference-panel" onClick={event => event.stopPropagation()}><header className="reference-panel-toolbar"><strong>引用记录 <span className="reference-record-id">{referenceTarget}</span></strong><button className="btn" onClick={() => setReferenceTarget(null)}>关闭</button></header><DataCardExpanded fields={Object.values(MOCK_FILE_RECORDS).flatMap(file => file.records).find(row => row.coordinate.id === referenceTarget)?.fields ?? []}/></section></div>}
    </main>
  </ObjectDraftHost>
}
createRoot(document.getElementById('root')!).render(<CellDesignPreview />)
