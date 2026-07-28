import { describe, expect, it } from 'vitest'
import { matchesTarget } from './index'
import type { FieldRenderer, ReadRenderContext } from './types'

const renderer: FieldRenderer = {
  id: 'test.foo',
  target: {
    kind: 'field-value',
    type: 'Foo',
    surfaces: ['table-cell'],
  },
  mount() {},
}

function context(type: string): ReadRenderContext {
  return {
    value: { kind: 'null' },
    type,
    nullable: type.endsWith('?'),
    surface: 'table-cell',
  }
}

describe('plugin field renderer matching', () => {
  it('matches a base type renderer for nullable values', () => {
    expect(matchesTarget(renderer, context('Foo?'))).toBe(true)
  })

  it('preserves exact matching for an explicitly nullable target', () => {
    const nullableRenderer = {
      ...renderer,
      target: { ...renderer.target, type: 'Foo?' },
    }

    expect(matchesTarget(nullableRenderer, context('Foo?'))).toBe(true)
    expect(matchesTarget(nullableRenderer, context('Foo'))).toBe(false)
  })
})
