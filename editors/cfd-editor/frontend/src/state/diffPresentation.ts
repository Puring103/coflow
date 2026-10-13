import type { ProjectRecordDiff } from '../bindings/ProjectRecordDiff'
import type { ProjectDiffDisplay } from '../bindings/ProjectDiffDisplay'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { CfdValue } from '../bindings/CfdValue'
import { dictKeyPathText, presentationValue } from '../wire'

/** 将快照的端点展示元数据接入普通表格、记录共用的字段注解。 */
export function diffAnnotation(display?: ProjectDiffDisplay): FieldAnnotation | null {
  if (!display) return null
  return {
    label: display.label ?? undefined,
    description: display.description ?? undefined,
    declared_type: null, ref_target_type: null, enum_type: null, enum_int_value: null,
    enum_is_flag: false, nullable: false, read_only: false,
    item_annotation: null, object_type: null, polymorphic_types: [], field_order: [],
    children: Object.fromEntries(Object.entries(display.children).map(([name, child]) => [name, diffAnnotation(child)!])),
  }
}

export function changedRecordFields(record: ProjectRecordDiff): Set<string> {
  // 维度列名本身带方括号，必须先匹配完整快照字段名，再判断嵌套路径。
  const fields = new Set([...(record.before?.values ?? []), ...(record.after?.values ?? [])].map(item => item.path))
  return new Set([...fields].filter(name => record.change !== 'modified'
    || record.fields.some(field => field.path === name || field.path.startsWith(`${name}.`) || field.path.startsWith(`${name}[`))))
}

/** 在真实值树上收集祖先，避免误解析含点号、括号或转义字符的字典键。 */
export function expandedDiffPaths(record: ProjectRecordDiff): Set<string> {
  const paths = new Set<string>()
  const changed = record.fields.map(field => field.path)
  const visit = (path: string, wrapped: CfdValue) => {
    const value = presentationValue(wrapped)
    if (value.kind !== 'object' && value.kind !== 'array' && value.kind !== 'dict') return
    if (record.change === 'modified' && !changed.some(change =>
      change === path || change.startsWith(`${path}.`) || change.startsWith(`${path}[`)
      || path.startsWith(`${change}.`) || path.startsWith(`${change}[`))) return
    paths.add(path)
    if (value.kind === 'object') {
      for (const [name, child] of Object.entries(value.value.fields)) visit(`${path}.${name}`, child)
    } else if (value.kind === 'array') {
      value.value.forEach((child, index) => visit(`${path}[${index}]`, child))
    } else {
      for (const [key, child] of value.value) visit(`${path}[${dictKeyPathText(key)}]`, child)
    }
  }
  for (const snapshot of [record.before, record.after]) {
    for (const field of snapshot?.values ?? []) visit(field.path, field.value)
  }
  return paths
}
