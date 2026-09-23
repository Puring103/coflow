import type { FileRecords } from '../bindings/FileRecords'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordRow } from '../bindings/RecordRow'
import { annotationChildren, type FieldValue } from '../wire'

// 缓存只依赖不可变文件快照对象，不用相对路径和版本冒充跨会话身份。
const graphSupportCache = new WeakMap<FileRecords, boolean>()

export function graphSupportForFile(file: FileRecords): boolean {
  const cached = graphSupportCache.get(file)
  if (cached !== undefined) return cached
  const supported = recordsSupportGraph(file.records)
  graphSupportCache.set(file, supported)
  return supported
}

export function recordsSupportGraph(records: readonly RecordRow[]): boolean {
  return records.some(record => record.fields.some(field => (
    annotationContainsReference(field.annotation) || valueContainsReference(field.value)
  )))
}

/**
 * Top-level field names that carry references (directly, or as array/dict
 * elements). These are the fields that can render as graph edges, so the
 * view editor offers them as selectable relations — derived from record data
 * so it works even before the graph itself has been loaded.
 */
export function relationFieldNames(records: readonly RecordRow[]): string[] {
  const names = new Set<string>()
  for (const record of records) {
    for (const field of record.fields) {
      if (annotationContainsReference(field.annotation) || valueContainsReference(field.value)) {
        names.add(field.name)
      }
    }
  }
  return [...names]
}

function annotationContainsReference(annotation: FieldAnnotation | null | undefined): boolean {
  if (!annotation) return false
  if (annotation.ref_target_type) return true
  if (annotationContainsReference(annotation.item_annotation)) return true
  return annotationChildren(annotation).some(annotationContainsReference)
}

function valueContainsReference(value: FieldValue): boolean {
  switch (value.kind) {
    case 'ref':
      return true
    case 'array':
      return value.value.some(valueContainsReference)
    case 'dict':
      return value.value.some(([, child]) => valueContainsReference(child))
    case 'object':
      return Object.values(value.value.fields).some(child => (
        child ? valueContainsReference(child) : false
      ))
    default:
      return false
  }
}
