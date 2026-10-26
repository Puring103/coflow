import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { FieldCell } from '../bindings/FieldCell'
import {
  annotationChild, dictKeyPathText, fieldPathDictKey, fieldPathField, fieldPathIndex, objectFieldCells,
  presentationValue, replacePresentationValue, type FieldPathSegment, type FieldValue,
} from '../wire'
import { collectionShapeForDeclaredType } from '../value/fieldValue'

export interface RelationPort {
  id: string
  path: FieldPathSegment[]
  targetType: string
  append: boolean
  readOnly: boolean
  nullable: boolean
  value: FieldValue
}

export interface RelationProjection {
  ports: RelationPort[]
  expanded: Set<string>
  byId: ReadonlyMap<string, RelationPort>
  appendPaths: ReadonlySet<string>
}

const projections = new WeakMap<FieldCell[], RelationProjection>()

// 端口保留结构化写入路径；显示路径只用于匹配连线，不能反向解析为配置路径。
export function relationPorts(fields: FieldCell[]): RelationProjection {
  const cached = projections.get(fields)
  if (cached) return cached
  const ports: RelationPort[] = []
  const expanded = new Set<string>()
  function visit(original: FieldValue, annotation: FieldAnnotation | null | undefined,
    path: FieldPathSegment[], id: string, ancestors: string[], readOnly: boolean) {
    const shown = presentationValue(original)
    const value = shown.kind === 'option_none'
      ? collectionShapeForDeclaredType(annotation?.declared_type ?? undefined) ?? shown : shown
    readOnly ||= !!annotation?.read_only
    if (annotation?.ref_target_type) {
      ports.push({ id, path, targetType: annotation.ref_target_type, append: false, readOnly, nullable: !!annotation.nullable, value: original })
      ancestors.forEach(parent => expanded.add(parent))
      return
    }
    const parents = [...ancestors, id]
    if (value.kind === 'object') {
      for (const field of objectFieldCells(value, annotation)) {
        visit(field.value, field.annotation, [...path, fieldPathField(field.name)], `${id}.${field.name}`, parents, readOnly)
      }
    } else if (value.kind === 'array') {
      value.value.forEach((item, index) => visit(item, annotationChild(annotation, index) ?? annotation?.item_annotation,
        [...path, fieldPathIndex(index)], `${id}[${index}]`, parents, readOnly))
      const item = annotation?.item_annotation
      if (item?.ref_target_type) {
        ports.push({ id: `${id}[+]`, path, targetType: item.ref_target_type, append: true,
          readOnly: readOnly || !!item.read_only, nullable: !!item.nullable, value: { kind: 'option_none' } })
        parents.forEach(parent => expanded.add(parent))
      }
    } else if (value.kind === 'dict') {
      for (const [key, item] of value.value) {
        const text = dictKeyPathText(key)
        visit(item, annotationChild(annotation, text) ?? annotation?.item_annotation,
          [...path, fieldPathDictKey(text)], `${id}[${text}]`, parents, readOnly)
      }
    }
  }
  for (const field of fields) visit(field.value, field.annotation, [fieldPathField(field.name)], field.name, [], false)
  const result = { ports, expanded, byId: new Map(ports.map(port => [port.id, port])),
    appendPaths: new Set(ports.filter(port => port.append).map(port => JSON.stringify(port.path))) }
  projections.set(fields, result)
  return result
}

export function graphCardFields(
  fields: FieldCell[],
  visible: ReadonlySet<string> | undefined,
  enabledRelations: ReadonlySet<string>,
): FieldCell[] {
  if (!visible) return fields
  // 当前选中的关系字段必须保留实体行，列表项连线才能锚定到对应元素。
  return fields.filter(field => visible.has(field.name) || enabledRelations.has(field.name))
}

export function relationValue(port: RelationPort, key: string): FieldValue {
  const reference: FieldValue = { kind: 'ref', value: key }
  return port.value.kind === 'option_none' && !port.nullable
    ? reference : replacePresentationValue(port.value, reference)
}
