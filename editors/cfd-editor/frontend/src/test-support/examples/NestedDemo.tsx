import { useState, useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { DataCardExpanded } from '../../features/record-editing/components/DataCard'
import { ObjectDraftHost } from '../../features/record-editing/components/ObjectDraftHost'
import { EditorLookupController } from '../../features/workspace/state/editorLookups'
import type { CfdValue as V } from '../../bindings/CfdValue'
import type { CfdDictKey } from '../../bindings/CfdDictKey'
import type { FieldAnnotation as A } from '../../bindings/FieldAnnotation'
import type { FieldCell } from '../../bindings/FieldCell'
import type { FieldPathSegment } from '../../wire'
import '../../style.css'
import './nested-demo.css'

const str = (value: string): V => ({ kind: 'string', value })
const num = (value: number): V => ({ kind: 'int', value: BigInt(value) })
const ref = (value: string): V => ({ kind: 'ref', value })
const obj = (actual_type: string, fields: Record<string, V>): V => ({ kind: 'object', value: { actual_type, fields } })
const arr = (...value: V[]): V => ({ kind: 'array', value })
const dict = (values: Record<string, V>): V => ({ kind: 'dict', value: Object.entries(values).map(([key, value]) => [{ kind: 'string', value: key }, value]) })
const names: Record<string, string> = { timber: '硬木', stone: '石料', scout: '侦察营地' }
const references: Record<string, Record<string, V>> = {
  timber: { 名称: str('硬木'), 描述: str('用于建筑支撑与工具制作的基础材料。'), 堆叠上限: num(999), 来源: arr(str('林地采集'), str('贸易站')), 关联设施: ref('scout') },
  stone: { 名称: str('石料'), 描述: str('用于地基和防御工事。'), 堆叠上限: num(500), 来源: arr(str('采石场'), str('废墟回收')) },
  scout: { 名称: str('侦察营地'), 等级: num(2), 储备: dict({ 硬木: num(40), 石料: num(25) }), 奖励: obj('Reward', { 物品: ref('timber'), 数量: num(8) }) },
}
const variants: Record<string, V> = { 等级条件: obj('等级条件', { 最低等级: num(3) }), 声望条件: obj('声望条件', { 阵营: str('探索者'), 最低声望: num(100) }) }
const initial: Record<string, V>[] = ['前哨站建设', '林地补给线', '山谷防御'].map((name, i) => ({
  名称: str(name), 启用: { kind: 'bool', value: i !== 2 }, 描述: str('建立稳定的补给与防御网络。完成各阶段后开放后续探索。'),
  解锁条件: obj('全部满足', { 等级条件: obj('等级条件', { 最低等级: num(3 + i) }), 任一满足: arr(obj('完成任务', { 前置设施: ref('scout') }), obj('持有物品', { 物品: ref('timber'), 数量: num(20) })) }),
  建设阶段: arr(obj('Stage', { 名称: str('清理场地'), 消耗: dict({ 体力: num(10), 工具: num(2) }), 奖励: obj('Reward', { 物品: ref('stone'), 数量: num(5), 附加奖励: { kind: 'option_none' } }) }), obj('Stage', { 名称: str('搭建营地'), 消耗: dict({ 硬木: num(30), 石料: num(15) }), 奖励: obj('Reward', { 物品: ref('timber'), 数量: num(8), 附加奖励: { kind: 'option_some', value: obj('Bonus', { 经验: num(100), 标签: arr(str('首次完成'), str('探索')) }) } }) })),
  区域配置: dict({ 森林: obj('Area', { 刷新间隔: num(60), 掉落组: arr(obj('Drop', { 物品: ref('timber'), 概率: { kind: 'float', value: 0.75 } }), obj('Drop', { 物品: ref('stone'), 概率: { kind: 'float', value: 0.25 } })) }), 荒原: obj('Area', { 刷新间隔: num(120), 掉落组: arr() }) }),
  结算方式: { kind: 'enum', value: { enum_name: 'Settlement', variant: '按阶段', value: 0n } }, 前置设施: ref('scout'), 备注: str(''), 可选奖励: { kind: 'option_none' }, 标签: arr(),
}))
function summary(v: V): string {
  switch (v.kind) {
    case 'string': return v.value || '空文本'
    case 'int': case 'float': return String(v.value)
    case 'bool': return v.value ? '已启用' : '已停用'
    case 'ref': return names[v.value] || v.value
    case 'option_none': return '未设置'
    case 'option_some': return summary(v.value)
    case 'array': return v.value.length ? v.value.slice(0, 2).map(summary).join('、') + ` · ${v.value.length} 项` : '0 项'
    case 'dict': return v.value.slice(0, 2).map(([k, value]) => `${k.kind === 'enum' ? k.value.variant : k.value}：${summary(value)}`).join(' · ') || '0 项'
    case 'object': return v.value.fields.名称 ? summary(v.value.fields.名称) : Object.entries(v.value.fields).slice(0, 2).map(([k, value]) => `${k} ${summary(value)}`).join(' · ')
    case 'enum': return v.value.variant || String(v.value.value)
    case 'formatted_string': return v.value.rendered
  }
}
function annotation(v: V): A {
  const children: Record<string, A> = {}
  if (v.kind === 'object') for (const [k, value] of Object.entries(v.value.fields)) children[k] = annotation(value)
  if (v.kind === 'array') v.value.forEach((value, i) => { children[i] = annotation(value) })
  if (v.kind === 'dict') v.value.forEach(([k, value]) => { children[String(k.value)] = annotation(value) })
  return { enum_int_value: null, declared_type: null, ref_target_type: v.kind === 'ref' ? 'Resource' : null, enum_type: v.kind === 'enum' ? 'Settlement' : null, enum_is_flag: false, nullable: v.kind.startsWith('option'), read_only: false, item_annotation: null, polymorphic_types: v.kind === 'object' && v.value.actual_type in variants ? Object.keys(variants) : [], object_type: v.kind === 'object' ? v.value.actual_type : null, field_order: v.kind === 'object' ? Object.keys(v.value.fields) : [], children }
}
const cells = (fields: Record<string, V>): FieldCell[] => Object.entries(fields).map(([name, value]) => ({ name, value, missing: false, annotation: annotation(value) }))
const lookups = new EditorLookupController({
  getEnumVariants: async () => [{ name: '按阶段', value: 0n, label: '按阶段', description: null }, { name: '全部完成', value: 1n, label: '全部完成', description: null }], getRefTargets: async () => Object.entries(names).map(([id, short_name]) => ({ short_name, coordinate: { id, actual_type: 'Resource' }, file_path: 'example.cfd' })),
  makeDefaultObject: async (_session, type) => structuredClone(variants[type] || obj(type, {})), createRecordDraft: async () => { throw new Error('示例不创建记录') },
})
lookups.adopt({ sessionId: 1, revision: 1 })
// 复用真实字段树和输入组件，写入仅更新示例内存中的嵌套值。
function replaceAt(value: V, path: FieldPathSegment[], next: V): V {
  if (!path.length) return next
  if (value.kind === 'option_some') return { ...value, value: replaceAt(value.value, path, next) }
  const [head, ...tail] = path
  if (value.kind === 'object') return { ...value, value: { ...value.value, fields: { ...value.value.fields, [String(head.value)]: replaceAt(value.value.fields[String(head.value)], tail, next) } } }
  if (value.kind === 'array') return { ...value, value: value.value.map((item, i) => i === Number(head.value) ? replaceAt(item, tail, next) : item) }
  if (value.kind === 'dict') return { ...value, value: value.value.map(([key, item]): [CfdDictKey, V] => [key, String(key.value) === String(head.value) ? replaceAt(item, tail, next) : item]) }
  return value
}
function Demo() {
  const [records, setRecords] = useState(initial)
  const [row, setRow] = useState(0)
  const [field, setField] = useState('建设阶段')
  const [popup, setPopup] = useState(false)
  const [history, setHistory] = useState<string[]>([])
  const [technical, setTechnical] = useState(false)
  const [notice, setNotice] = useState('修改仅保存在此示例，刷新即可恢复')
  const [filter, setFilter] = useState('')
  useEffect(() => { const escape = (e: KeyboardEvent) => { if (e.key !== 'Escape') return; if (history.length) setHistory(h => h.slice(0, -1)); else setPopup(false) }; window.addEventListener('keydown', escape); return () => window.removeEventListener('keydown', escape) }, [history])
  const current = records[row]
  const edit = (path: FieldPathSegment[], next: V) => { setRecords(all => all.map((record, i) => i === row ? (replaceAt(obj('Record', record), path, next) as V & { kind: 'object' }).value.fields : record)); setNotice('已更新示例 · 表格与详情同步') }
  const preview = history[history.length - 1]
  const copy = async (text: string) => { try { await navigator.clipboard.writeText(text); setNotice('已复制') } catch { setNotice('浏览器未允许复制，请选择文本复制') } }
  return <ObjectDraftHost lookups={lookups} generationKey="1:1" sessionId={1} onOpenReference={(_type, id) => setHistory(h => [...h, id])}>
    <div className={`nested-demo ${technical ? 'technical' : ''}`}>
      <header className="nd-top"><div><span className="nd-brand">C</span><strong>数据工作台</strong><span className="nd-muted">/ 交互示例</span></div><div><button className="btn" onClick={() => document.documentElement.dataset.theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'}>明暗切换</button><button className="btn" onClick={() => setTechnical(!technical)}>{technical ? '隐藏' : '显示'}技术信息</button><button className="btn" onClick={() => { setRecords(structuredClone(initial)); setNotice('示例已重置') }}>重置</button></div></header>
      <div className="nd-intro"><div><span className="nd-eyebrow">嵌套数据 / 轻量展示</span><h1>前哨站与探索</h1><p>先读摘要，再展开细节。点击表格中的值体验局部编辑，点击引用名称查看关联记录。</p></div><span className="nd-count">3 条记录 · 11 个字段</span></div>
      <main className="nd-workspace"><section className="nd-table-area"><div className="nd-toolbar"><strong>表格</strong><span className="nd-muted">业务摘要</span></div><div className="nd-table-scroll"><table className="nd-table"><thead><tr><th>记录</th>{['解锁条件', '建设阶段', '区域配置', '前置设施'].map(k => <th key={k}>{k}</th>)}</tr></thead><tbody>{records.map((record, i) => <tr key={i} className={row === i ? 'active' : ''}><th><button onClick={() => setRow(i)}>{summary(record.名称)}</button><small>{summary(record.启用)}</small></th>{['解锁条件', '建设阶段', '区域配置', '前置设施'].map(k => <td key={k}><button className={row === i && field === k ? 'selected' : ''} onClick={() => { setRow(i); setField(k); if (record[k].kind === 'ref') setHistory([(record[k] as V & { kind: 'ref' }).value]); else setPopup(true) }}><span>{summary(record[k])}</span><span className="nd-expand">{record[k].kind === 'ref' ? '↗' : '↗ 展开'}</span></button></td>)}</tr>)}</tbody></table></div><div className="nd-guide"><strong>试试这些结构</strong><p>建设阶段 → 第二项 → 奖励 → 附加奖励</p><p>区域配置 → 森林 → 掉落组 → 物品</p><p>解锁条件 → 任一满足 → 完成任务</p><div className="nd-empty-states"><span>备注 <b>空文本</b></span><span>可选奖励 <b>未设置</b></span><span>标签 <b>0 项</b></span></div></div></section>
      <aside className="nd-inspector"><div className="nd-panel-heading"><span className="nd-eyebrow">记录详情</span><h2>{summary(current.名称)}</h2><small>outpost_{row + 1}</small><input aria-label="筛选字段" placeholder="筛选字段…" value={filter} onChange={e => setFilter(e.target.value)} /></div><div className="nd-fields">{Object.entries(current).filter(([k]) => k.includes(filter)).map(([k, value]) => <details key={`${row}:${k}`} open={k === field || k === '名称' || undefined}><summary onClick={() => setField(k)}><strong>{k}</strong><span>{summary(value)}</span></summary><div className="nd-field-content">{value.kind === 'ref' ? <button className="nd-reference" onClick={() => setHistory([value.value])}>{summary(value)} ↗</button> : <DataCardExpanded summarizeValue={summary} fields={cells({ [k]: value })} onEdit={edit} expandAlongPath={k} />}</div></details>)}</div></aside></main><footer role="status">{notice}</footer>
      {popup && <div className="nd-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setPopup(false) }}><section role="dialog" aria-label="单元格详情" className="nd-popover"><div className="nd-panel-heading"><div className="nd-pop-actions"><span className="nd-eyebrow">{summary(current.名称)} / {field}</span><button className="btn" aria-label="关闭单元格详情" onClick={() => setPopup(false)}>×</button></div><h2>{field}</h2><p>{summary(current[field])}</p></div><div className="nd-pop-body"><DataCardExpanded summarizeValue={summary} key={`${row}:${field}`} fields={cells({ [field]: current[field] })} onEdit={edit} expandAlongPath={field} /></div><div className="nd-pop-footer"><button className="btn" onClick={() => copy(summary(current[field]))}>复制摘要</button><button className="btn btn-primary" onClick={() => { setPopup(false); setField(field) }}>在详情中查看</button></div></section></div>}
      {preview && <div className="nd-reference-overlay" onMouseDown={e => { if (e.target === e.currentTarget) setHistory([]) }}><section className="nd-reference-panel" role="dialog" aria-label="引用预览"><div className="nd-panel-heading"><div className="nd-pop-actions"><span className="nd-eyebrow">引用预览 · 只读</span><button className="btn" aria-label="关闭引用预览" onClick={() => setHistory([])}>×</button></div>{history.length > 1 && <button className="btn" onClick={() => setHistory(h => h.slice(0, -1))}>← 返回</button>}<h2>{names[preview] || preview}</h2><small>{preview}</small></div><div className="nd-pop-body"><DataCardExpanded summarizeValue={summary} fields={cells(references[preview] || {})} /></div><div className="nd-pop-footer"><button className="btn" onClick={() => copy(preview)}>复制 ID</button><button className="btn" onClick={() => copy(names[preview] || preview)}>复制名称</button>{preview !== 'scout' && <button className="btn" onClick={() => setHistory(h => [...h, 'scout'])}>查看关联营地 ↗</button>}</div></section></div>}
    </div>
  </ObjectDraftHost>
}
createRoot(document.getElementById('root')!).render(<Demo />)
