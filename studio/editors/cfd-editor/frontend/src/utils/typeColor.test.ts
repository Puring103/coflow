import { describe, expect, it } from 'vitest'
import { fieldTypeColor, typeColor } from './typeColor'

describe('fieldTypeColor', () => {
  it('uses stable semantic colors for primitive types and nullable forms', () => {
    expect(fieldTypeColor('bool')).toBe(fieldTypeColor('bool?'))
    expect(fieldTypeColor('int')).toBe(fieldTypeColor('float'))
    expect(fieldTypeColor('string')).not.toBe(fieldTypeColor('int'))
  })

  it('uses the named type color for schema types', () => {
    expect(fieldTypeColor('Item')).toBe(typeColor('Item'))
    expect(fieldTypeColor('Item?')).toBe(typeColor('Item'))
    expect(fieldTypeColor('&Item')).toBe(typeColor('Item'))
  })
})
