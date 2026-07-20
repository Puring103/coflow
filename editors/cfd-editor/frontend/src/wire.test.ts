import { describe, expect, it } from 'vitest'
import type { FieldAnnotation } from './bindings/FieldAnnotation'
import { annotationChildren } from './wire'

describe('annotationChildren', () => {
  it('accepts annotations whose empty children map was omitted by serde', () => {
    const annotation = { declared_type: 'ref<Item>' } as unknown as FieldAnnotation

    expect(annotationChildren(annotation)).toEqual([])
  })

  it('returns defined nested annotations', () => {
    const child = { ref_target_type: 'Item' } as unknown as FieldAnnotation
    const annotation = {
      children: { target: child, empty: undefined },
    } as unknown as FieldAnnotation

    expect(annotationChildren(annotation)).toEqual([child])
  })
})
