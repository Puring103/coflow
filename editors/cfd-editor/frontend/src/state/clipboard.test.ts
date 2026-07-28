import { describe, expect, it } from 'vitest'
import type { CfdValue } from '../bindings/CfdValue'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import { fieldPathField } from '../wire'
import { parseTsv, planPaste, serializeCellMatrix, type PasteCell } from './clipboard'

const coordinate = { actual_type: 'Item', key: 'one' }
const annotation = (overrides: Partial<FieldAnnotation> = {}): FieldAnnotation => ({
  spread_info: null,
  ref_target_file: null,
  enum_int_value: null,
  declared_type: 'string',
  ref_target_type: null,
  enum_type: null,
  nullable: false,
  read_only: false,
  item_annotation: null,
  polymorphic_types: [],
  object_type: null,
  field_order: [],
  children: {},
  ...overrides,
})
const cell = (field: string, overrides: Partial<PasteCell> = {}): PasteCell => ({
  coordinate,
  fieldPath: [fieldPathField(field)],
  annotation: annotation(),
  value: { kind: 'string', value: '' },
  writable: true,
  ...overrides,
})

describe('TSV clipboard codec', () => {
  it('round-trips tabs, quotes, newlines, CRLF, and trailing empty cells', async () => {
    const cells = [[
      { coordinate, fieldPath: [fieldPathField('a')] },
      { coordinate, fieldPath: [fieldPathField('b')] },
    ]]
    const text = await serializeCellMatrix(cells, async (_coordinate, path) => (
      path[0].value === 'a' ? 'a\t"b"\nline' : ''
    ))

    expect(parseTsv(text.replace('\nline', '\r\nline'))).toEqual([['a\t"b"\r\nline', '']])
    expect(() => parseTsv('"unterminated')).toThrow(/not closed/)
    expect(() => parseTsv('"ok"tail')).toThrow(/trailing/)
  })
})

describe('paste planner', () => {
  it('accepts older scalar annotations that omit optional object metadata', async () => {
    const target = cell('name', {
      annotation: { declared_type: 'string', read_only: false } as FieldAnnotation,
    })
    const result = await planPaste([['updated']], [[target]], {
      mode: 'replace',
      parse: async (_coordinate, _path, text) => ({ kind: 'string', value: text }),
    })
    expect(result.ok).toBe(true)
  })

  it('broadcasts one source cell but parses independently for mixed targets', async () => {
    const seen: string[] = []
    const result = await planPaste([['7']], [[cell('name'), cell('count')]], {
      mode: 'replace',
      parse: async (_coordinate, path, text) => {
        const field = path[0].value as string
        seen.push(field)
        return field === 'count'
          ? { kind: 'int', value: BigInt(text) }
          : { kind: 'string', value: text }
      },
    })

    expect(result.ok).toBe(true)
    expect(seen).toEqual(['name', 'count'])
    if (result.ok) expect(result.writes.map(write => write.new_value.kind)).toEqual(['string', 'int'])
  })

  it('parses a single array cell as a full array and then as one item', async () => {
    const target = cell('values', {
      annotation: annotation({ item_annotation: annotation({ declared_type: 'int' }) }),
      value: { kind: 'array', value: [] },
    })
    const parse = async (_coordinate: typeof coordinate, path: ReturnType<typeof fieldPathField>[], text: string): Promise<CfdValue> => {
      if (path[path.length - 1]?.kind === 'index') return { kind: 'int', value: BigInt(text) }
      if (text.startsWith('[')) return { kind: 'array', value: [{ kind: 'int', value: 1n }] }
      throw new Error('not an array')
    }

    const full = await planPaste([['[1]']], [[target]], { mode: 'replace', parse })
    const item = await planPaste([['2']], [[target]], { mode: 'replace', parse })
    expect(full.ok && full.writes[0].new_value).toEqual({ kind: 'array', value: [{ kind: 'int', value: 1n }] })
    expect(item.ok && item.writes[0].new_value).toEqual({ kind: 'array', value: [{ kind: 'int', value: 2n }] })
  })

  it('assembles object fields in explicit schema order, including complex fields', async () => {
    const target = cell('stats', {
      annotation: annotation({ object_type: 'Stats', field_order: ['labels', 'hp'] }),
      value: { kind: 'object', value: { actual_type: 'Stats', fields: {} } },
    })
    const result = await planPaste([['["rare"]', '10']], [[target]], {
      mode: 'replace',
      parse: async (_coordinate, path, text) => path[path.length - 1]?.value === 'hp'
        ? { kind: 'int', value: BigInt(text) }
        : { kind: 'array', value: [{ kind: 'string', value: 'rare' }] },
    })

    expect(result.ok).toBe(true)
    if (result.ok) expect(result.writes[0].new_value).toEqual({
      kind: 'object',
      value: {
        actual_type: 'Stats',
        fields: {
          labels: { kind: 'array', value: [{ kind: 'string', value: 'rare' }] },
          hp: { kind: 'int', value: 10n },
        },
      },
    })
  })

  it('returns every parse error without producing writes', async () => {
    const result = await planPaste([['bad', 'also bad']], [[cell('a'), cell('b')]], {
      mode: 'replace',
      parse: async () => { throw new Error('bad') },
    })

    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.errors).toHaveLength(2)
  })
})
