import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { DictKey } from '../wire'
import {
  annotationDeclaredType,
  annotationEnumType,
  annotationPolymorphicTypes,
  annotationRefTargetType,
} from '../wire'
import { scalarDefaultForDeclaredType } from '../value/fieldValue'

export function dictKeyTemplate(annotation?: FieldAnnotation): DictKey | null {
  const enumType = annotationEnumType(annotation)
  if (enumType) {
    return { kind: 'enum', value: { enum_name: enumType, variant: null, value: 0n } }
  }
  switch (annotationDeclaredType(annotation)) {
    case 'int': return { kind: 'int', value: 0n }
    case 'string': return { kind: 'string', value: '' }
    default: return null
  }
}

export function collectionObjectDraftForAnnotation(
  annotation: FieldAnnotation | undefined,
  collectionIsEmpty: boolean,
): { actualType: string, polymorphicTypes: string[] } | null {
  const draft = objectDraftForAnnotation(annotation)
  if (!draft) return null
  return collectionIsEmpty || draft.polymorphicTypes.length >= 2 ? draft : null
}

function objectDraftForAnnotation(annotation?: FieldAnnotation): {
  actualType: string
  polymorphicTypes: string[]
} | null {
  if (!annotation || annotationRefTargetType(annotation) || annotationEnumType(annotation)) return null
  const polymorphicTypes = annotationPolymorphicTypes(annotation)
  const declaredType = annotationDeclaredType(annotation)
  const actualType = polymorphicTypes[0] ?? declaredType?.replace(/\?$/, '')
  if (!actualType || scalarDefaultForDeclaredType(actualType) !== null) return null
  return { actualType, polymorphicTypes }
}
