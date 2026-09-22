import { describe, expect, it } from 'vitest'
import type { DiagnosticContext } from './bindings/DiagnosticContext'
import type { FieldAnnotation } from './bindings/FieldAnnotation'
import {
  annotationChildren,
  applyCreatedValue,
  diagnosticDisplayMessage,
  nullValue,
  objectFieldCells,
  optionalState,
  presentationValue,
  replacePresentationValue,
  type DiagnosticItem,
} from './wire'

function context(kind: string, values: Partial<DiagnosticContext> = {}): DiagnosticContext {
  return {
    kind,
    name: null,
    expression: null,
    quantifier: null,
    binding: null,
    item: null,
    dimension: null,
    variant: null,
    ...values,
  }
}

function diagnostic(contexts: DiagnosticItem['contexts'] = []): DiagnosticItem {
  return {
    id: 'custom-message',
    severity: 'error',
    code: 'CHECK-001',
    stage: 'CHECK',
    message: 'custom message',
    target: { kind: 'none' },
    contexts,
  } as DiagnosticItem
}

describe('diagnostic display message', () => {
  it('renders structured contexts without changing the diagnostic message', () => {
    const item = diagnostic([
      context('check', { name: 'ItemRules' }),
      context('when', { expression: 'enabled' }),
      context('quantifier', { quantifier: 'all', binding: 'item', item: 'sword' }),
      context('dimension', { dimension: 'language', variant: 'zh-CN' }),
      context('future'),
    ])

    expect(diagnosticDisplayMessage(item)).toBe(
      [
        'custom message',
        '上下文: check ItemRules',
        '上下文: 在 when enabled 内',
        '上下文: 绑定 item 位于 sword',
        '上下文: language=zh-CN',
        '上下文: future',
      ].join('\n'),
    )
    expect(item.message).toBe('custom message')
  })

})

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

describe('wrapped values', () => {
  it('wraps only explicit Option creation targets', () => {
    const created = { kind: 'object', value: { actual_type: 'Damage', fields: {} } } as const

    expect(applyCreatedValue(false, created)).toEqual(created)
    expect(applyCreatedValue(true, created)).toEqual({
      kind: 'option_some',
      value: created,
    })
  })

  it('presents optional payloads without losing their outer variant on edit', () => {
    const some = { kind: 'option_some', value: { kind: 'int', value: 1n } } as const

    expect(presentationValue(some)).toEqual({ kind: 'int', value: 1n })
    expect(replacePresentationValue(some, { kind: 'int', value: 2n })).toEqual({
      kind: 'option_some', value: { kind: 'int', value: 2n },
    })
    expect(replacePresentationValue(some, nullValue())).toEqual({ kind: 'option_none' })
    expect(replacePresentationValue(nullValue(), { kind: 'bool', value: true })).toEqual({
      kind: 'option_some', value: { kind: 'bool', value: true },
    })
  })

  it('reports the single declared Option state', () => {
    expect(optionalState(nullValue(), true)).toBe('none')
    expect(optionalState({ kind: 'option_some', value: { kind: 'int', value: 1n } }, true)).toBe('some')
    expect(optionalState({ kind: 'int', value: 1n }, false)).toBeNull()
  })
})

describe('objectFieldCells', () => {
  it('projects missing schema fields in schema order and preserves extra stored fields', () => {
    const target = { ref_target_type: 'Item' } as unknown as FieldAnnotation
    const annotation = {
      field_order: ['target', 'name'],
      children: { target },
    } as unknown as FieldAnnotation
    const value = {
      kind: 'object',
      value: {
        actual_type: 'Holder',
        fields: {
          name: { kind: 'string', value: 'holder' },
          extra: { kind: 'bool', value: true },
        },
      },
    } as const

    expect(objectFieldCells(value, annotation)).toEqual([
      { name: 'target', value: nullValue(), missing: true, annotation: target },
      { name: 'name', value: { kind: 'string', value: 'holder' }, missing: false, annotation: null },
      { name: 'extra', value: { kind: 'bool', value: true }, missing: false, annotation: null },
    ])
  })
})
