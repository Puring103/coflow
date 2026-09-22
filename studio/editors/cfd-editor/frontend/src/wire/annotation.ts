import type { FieldCell } from '../bindings/FieldCell'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'

/** 注解访问器唯一实现：`annotation*` 为权威实现，`cell*` 只做一层透传，避免 16 个手写重复。 */
export function annotationDeclaredType(annotation: FieldAnnotation | null | undefined): string | undefined {
  return annotation?.declared_type ?? undefined
}

export function annotationRefTargetType(annotation: FieldAnnotation | null | undefined): string | undefined {
  return annotation?.ref_target_type ?? undefined
}

export function annotationEnumType(annotation: FieldAnnotation | null | undefined): string | undefined {
  return annotation?.enum_type ?? undefined
}

export function annotationEnumIsFlag(annotation: FieldAnnotation | null | undefined): boolean {
  return !!annotation?.enum_is_flag
}

export function annotationNullable(annotation: FieldAnnotation | null | undefined): boolean {
  return !!annotation?.nullable
}

export function annotationReadOnly(annotation: FieldAnnotation | null | undefined): boolean {
  return !!annotation?.read_only
}

export function annotationItem(annotation: FieldAnnotation | null | undefined): FieldAnnotation | undefined {
  return annotation?.item_annotation ?? undefined
}

export function annotationKey(annotation: FieldAnnotation | null | undefined): FieldAnnotation | undefined {
  return annotation?.key_annotation ?? undefined
}

export function annotationPolymorphicTypes(annotation: FieldAnnotation | null | undefined): string[] {
  return annotation?.polymorphic_types ?? []
}

export function annotationChildren(annotation: FieldAnnotation | null | undefined): FieldAnnotation[] {
  return Object.values(annotation?.children ?? {}).filter(
    (child): child is FieldAnnotation => child !== undefined,
  )
}

export function annotationChild(
  annotation: FieldAnnotation | null | undefined,
  key: string | number,
): FieldAnnotation | undefined {
  return annotation?.children?.[String(key)] ?? undefined
}

export function cellDeclaredType(cell: FieldCell): string | undefined {
  return annotationDeclaredType(cell.annotation)
}

export function cellRefTargetType(cell: FieldCell): string | undefined {
  return annotationRefTargetType(cell.annotation)
}

export function cellEnumType(cell: FieldCell): string | undefined {
  return annotationEnumType(cell.annotation)
}

export function cellEnumIsFlag(cell: FieldCell): boolean {
  return annotationEnumIsFlag(cell.annotation)
}

export function cellNullable(cell: FieldCell): boolean {
  return annotationNullable(cell.annotation)
}

export function cellReadOnly(cell: FieldCell): boolean {
  return annotationReadOnly(cell.annotation)
}

export function cellItemAnnotation(cell: FieldCell): FieldAnnotation | undefined {
  return annotationItem(cell.annotation)
}
