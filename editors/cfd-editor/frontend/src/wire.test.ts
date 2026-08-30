import { describe, expect, it } from 'vitest'
import type { DiagnosticContext } from './bindings/DiagnosticContext'
import type { FieldAnnotation } from './bindings/FieldAnnotation'
import {
  annotationChildren,
  diagnosticDisplayMessage,
  nullValue,
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

function diagnostic(contexts?: DiagnosticItem['contexts']): DiagnosticItem {
  return {
    severity: 'error',
    code: 'CHECK-001',
    stage: 'CHECK',
    message: 'custom message',
    file_path: null,
    actual_type: null,
    record_key: null,
    field_path: null,
    ...(contexts === undefined ? {} : { contexts }),
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

  it('keeps legacy diagnostics unchanged when contexts are absent', () => {
    const item = diagnostic(undefined)

    expect(diagnosticDisplayMessage(item)).toBe('custom message')
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
  it('presents Option and Result payloads without losing their outer variant on edit', () => {
    const some = { kind: 'option_some', value: { kind: 'int', value: 1n } } as const
    const err = { kind: 'result_err', value: { kind: 'string', value: 'old' } } as const

    expect(presentationValue(some)).toEqual({ kind: 'int', value: 1n })
    expect(replacePresentationValue(some, { kind: 'int', value: 2n })).toEqual({
      kind: 'option_some', value: { kind: 'int', value: 2n },
    })
    expect(replacePresentationValue(err, { kind: 'string', value: 'new' })).toEqual({
      kind: 'result_err', value: { kind: 'string', value: 'new' },
    })
    expect(replacePresentationValue(some, nullValue())).toEqual({ kind: 'option_none' })
    expect(replacePresentationValue(nullValue(), { kind: 'bool', value: true })).toEqual({
      kind: 'option_some', value: { kind: 'bool', value: true },
    })
  })
})
