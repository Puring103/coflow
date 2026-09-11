import { describe, expect, it } from 'vitest'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { FieldCell } from '../bindings/FieldCell'
import { dictKeyPathText, type FieldValue } from '../wire'
import { graphCardFields, relationPorts, relationValue } from './GraphView.relations'

function annotation(extra: Partial<FieldAnnotation> = {}): FieldAnnotation {
  return { enum_int_value: null, declared_type: null, ref_target_type: null, enum_type: null,
    enum_is_flag: false, nullable: false, read_only: false, item_annotation: null,
    polymorphic_types: [], object_type: null, field_order: [], children: {}, ...extra }
}
const ref = annotation({ declared_type: 'Item', ref_target_type: 'Item' })
function field(value: FieldValue, meta: FieldAnnotation = ref, name = 'links'): FieldCell {
  return { name, value, annotation: meta, missing: false }
}

describe('graph relation ports', () => {
  it('gives each list element a replacement path and the tail an append path', () => {
    const result = relationPorts([field({ kind: 'array', value: [
      { kind: 'ref', value: 'A' }, { kind: 'ref', value: 'B' },
    ] }, annotation({ item_annotation: ref }))])
    expect(result.ports.map(port => port.id)).toEqual(['links[0]', 'links[1]', 'links[+]'])
    expect(result.ports[1].path).toEqual([{ kind: 'field', value: 'links' }, { kind: 'index', value: 1 }])
    expect(result.ports[2]).toMatchObject({ append: true, path: [{ kind: 'field', value: 'links' }] })
    expect(result.expanded).toEqual(new Set(['links']))
  })

  it('exposes an append port for empty and absent optional lists', () => {
    const meta = annotation({ declared_type: '[Item]?', nullable: true, item_annotation: ref })
    for (const value of [{ kind: 'array', value: [] }, { kind: 'option_none' }] as FieldValue[]) {
      const { ports } = relationPorts([field(value, meta)])
      expect(ports).toHaveLength(1)
      expect(ports[0]).toMatchObject({ id: 'links[+]', append: true })
      expect(relationValue(ports[0], 'B')).toEqual({ kind: 'ref', value: 'B' })
    }
  })

  it('expands ancestors and preserves structured paths through nested objects and lists', () => {
    const object = annotation({ children: { target: ref }, field_order: ['target'] })
    const { ports, expanded } = relationPorts([field({ kind: 'array', value: [
      { kind: 'object', value: { actual_type: 'Entry', fields: { target: { kind: 'ref', value: 'A' } } } },
    ] }, annotation({ item_annotation: object, read_only: true }))])
    expect(ports[0]).toMatchObject({ id: 'links[0].target', readOnly: true,
      path: [{ kind: 'field', value: 'links' }, { kind: 'index', value: 0 }, { kind: 'field', value: 'target' }] })
    expect(expanded).toEqual(new Set(['links', 'links[0]']))
  })

  it('uses canonical dictionary keys rather than parsing labels', () => {
    const key = { kind: 'string' as const, value: 'a"b\\c' }
    const { ports } = relationPorts([field({ kind: 'dict', value: [[key, { kind: 'ref', value: 'A' }]] },
      annotation({ item_annotation: ref }))])
    expect(ports[0].path[1]).toEqual({ kind: 'dict_key', value: dictKeyPathText(key) })
    expect(ports[0].id).toBe(`links[${dictKeyPathText(key)}]`)
  })

  it('applies visibility to relation fields without changing their ports', () => {
    const fields = [field({ kind: 'string', value: 'name' }, annotation(), 'name'),
      field({ kind: 'array', value: [] }, annotation({ item_annotation: ref }))]
    expect(graphCardFields(fields, new Set(), new Set())).toEqual([])
    expect(graphCardFields(fields, new Set(['name']), new Set()).map(field => field.name)).toEqual(['name'])
    expect(relationPorts(fields).ports.map(port => port.id)).toEqual(['links[+]'])
  })

  it('retains selected relation list rows while excluding other hidden fields', () => {
    const fields = [field({ kind: 'string', value: 'name' }, annotation(), 'name'),
      field({ kind: 'array', value: [{ kind: 'ref', value: 'A' }, { kind: 'ref', value: 'B' }] },
        annotation({ item_annotation: ref })),
      field({ kind: 'ref', value: 'C' }, ref, 'other')]
    const shown = graphCardFields(fields, new Set(), new Set(['links']))
    expect(shown.map(field => field.name)).toEqual(['links'])
    expect(relationPorts(shown).ports.map(port => port.id)).toEqual(['links[0]', 'links[1]', 'links[+]'])
    expect(relationPorts(shown).expanded).toEqual(new Set(['links']))
  })

  it('preserves optional wrappers and distinguishes required missing references', () => {
    const none: FieldValue = { kind: 'option_none' }
    const optional = relationPorts([field(none, annotation({ ...ref, nullable: true }))]).ports[0]
    const required = relationPorts([field(none)]).ports[0]
    expect(relationValue(optional, 'B')).toEqual({ kind: 'option_some', value: { kind: 'ref', value: 'B' } })
    expect(relationValue(required, 'B')).toEqual({ kind: 'ref', value: 'B' })
    const wrapped = relationPorts([field({ kind: 'option_some', value: { kind: 'ref', value: 'A' } },
      annotation({ ...ref, nullable: true }))]).ports[0]
    expect(relationValue(wrapped, 'B')).toEqual(relationValue(optional, 'B'))
  })
})
