import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordRow } from '../bindings/RecordRow'
import { annotationChildren, type FieldValue } from '../wire'

export function recordsSupportGraph(records: readonly RecordRow[]): boolean {
  return records.some(record => record.fields.some(field => (
    annotationContainsReference(field.annotation) || valueContainsReference(field.value)
  )))
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
