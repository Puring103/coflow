import { describe, expect, it } from 'vitest'
import { GraphRowGeometry } from './rowGeometry'
import type { FieldCell } from '../bindings/FieldCell'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import { relationPorts } from '../components/GraphView.relations'
import { countVisibleRows } from '../components/DataCard.geometry'

const annotation = (declared_type: string): FieldAnnotation => ({ declared_type, ref_target_type: null,
  enum_int_value: null, enum_type: null, enum_is_flag: false, nullable: false, read_only: false,
  item_annotation: null, polymorphic_types: [], object_type: null, field_order: [], children: {} })
const list = (count: number): FieldCell[] => [{ name: 'links', missing: false,
  value: { kind: 'array', value: Array.from({ length: count }, () => ({ kind: 'ref', value: 'same' })) },
  annotation: { ...annotation('[&Item]'), item_annotation: { ...annotation('&Item'), ref_target_type: 'Item' } } }]

describe('graph row geometry', () => {
  it('uses omitted annotation defaults through layout, relation projection and row geometry', () => {
    const fields: FieldCell[] = [
      { name: 'name', missing: false, value: { kind: 'string', value: '名称' }, annotation: { declared_type: 'string' } },
      { name: 'links', missing: false, value: { kind: 'array', value: [{ kind: 'ref', value: 'same' }] },
        annotation: { declared_type: '[&Item]', item_annotation: { declared_type: '&Item', ref_target_type: 'Item' } } },
      { name: 'dict', missing: false, value: { kind: 'dict', value: [[{ kind: 'string', value: 'key' }, { kind: 'int', value: 1n }]] },
        annotation: { declared_type: '{string: int}', item_annotation: { declared_type: 'int' } } },
    ]
    const relation = relationPorts(fields)
    expect(relation.ports.map(port => [port.id, port.readOnly, port.nullable])).toEqual([
      ['links[0]', false, false], ['links[+]', false, false],
    ])
    const expanded = new Set(['links', 'dict'])
    expect(countVisibleRows(fields, expanded)).toBe(5)
    const geometry = new GraphRowGeometry(fields, expanded, relation.appendPaths, true)
    expect(geometry.rows).toHaveLength(6)
    expect(geometry.rows.every(row => Number.isFinite(geometry.center(row.path)))).toBe(true)
  })

  it('keeps a fixed viewport bounded while all duplicate target anchors remain distinct', () => {
    const mounted: number[] = []
    for (const count of [1000, 10000]) {
      const geometry = new GraphRowGeometry(list(count), new Set(['links']), new Set(['[{"kind":"field","value":"links"}]']), true)
      expect(geometry.probes(100).length).toBeLessThan(6)
      geometry.measure(new Map([[geometry.rows[0].key, 25], [geometry.rows[1].key, 31], [`append:${geometry.rows[0].key}`, 24]]))
      const [first, end] = geometry.range(5000, 6000)
      const slices = geometry.slices(geometry.rows[0].key, first, end, new Set())
      mounted.push(slices.filter(slice => slice.visible).length)
      expect(slices.reduce((sum, slice) => sum + slice.height, 0)).toBe(count * 31)
      expect(geometry.center('links[999]') - geometry.center('links[0]')).toBe(999 * 31)
      expect(geometry.rows.length).toBe(count + 2)
    }
    expect(mounted[0]).toBe(mounted[1])
  })

  it('pins an active row and its ancestors independently of the viewport', () => {
    const geometry = new GraphRowGeometry(list(1000), new Set(['links']), new Set(), true)
    const active = geometry.byPath.get('links[900]')!
    const root = geometry.slices('[]', 1, 3, new Set([active]))
    expect(root.filter(slice => slice.visible)).toHaveLength(1)
    const items = geometry.slices(geometry.rows[0].key, 1, 3, new Set([active]))
    expect(items.filter(slice => slice.visible).map(slice => slice.first)).toEqual([0, 1, 900])
    expect(items.reduce((sum, slice) => sum + slice.height, 0)).toBe(geometry.bodyHeight - geometry.rowHeight(geometry.rows[0].key))
  })

  it('updates dynamic heights without changing structured dictionary paths or later row identity', () => {
    const field: FieldCell = { name: 'values', missing: false, annotation: annotation('{string: string}'), value: {
      kind: 'dict', value: [[{ kind: 'string', value: 'x[0]."y' }, { kind: 'string', value: 'long' }],
        [{ kind: 'string', value: 'tail' }, { kind: 'string', value: 'short' }]],
    } }
    const geometry = new GraphRowGeometry([field], new Set(['values']), new Set(), true)
    const first = geometry.rows[1], tail = geometry.rows[2]
    expect(JSON.parse(first.key)).toEqual([{ kind: 'field', value: 'values' }, { kind: 'dict_key', value: '"x[0].\\"y"' }])
    const before = geometry.center(tail.path)
    const old = geometry.rowHeight(first.key)
    geometry.measure(new Map([[first.key, 120]]))
    expect(geometry.center(tail.path)).toBe(before + 120 - old)
    expect(geometry.rows[2]).toBe(tail)
    const [start] = geometry.range(geometry.top(2), geometry.top(2) + 1)
    expect(start).toBe(2)
  })

  it('does not materialize inner collection rows through an absent outer Option', () => {
    const field: FieldCell = { name: 'values', missing: false, annotation: annotation('Option<Option<[int]>>'), value: { kind: 'option_none' } }
    const geometry = new GraphRowGeometry([field], new Set(['values']), new Set(), true)
    expect(geometry.rows).toHaveLength(1)
  })

  it('keeps an expanded active editor height local to its row', () => {
    const geometry = new GraphRowGeometry(list(1000), new Set(['links']), new Set(), true)
    const active = geometry.rows[4].key, ordinary = geometry.rows[5].key
    geometry.measure(new Map([[ordinary, 31]]))
    geometry.measure(new Map([[active, 140]]), undefined, new Set([active]))
    expect(geometry.rowHeight(active)).toBe(140)
    expect(geometry.rowHeight(ordinary)).toBe(31)
    geometry.measure(new Map([[ordinary, 33]]))
    expect(geometry.rowHeight(active)).toBe(140)
    geometry.measure(new Map([[active, 33]]))
    expect(geometry.rowHeight(active)).toBe(33)
  })

  it('ends array row identity on index shifts while retaining it for ordinary value changes and append', () => {
    const fields = list(3)
    fields[0].value = { kind: 'array', value: [1n, 2n, 3n].map(value => ({ kind: 'int', value })) }
    const build = (values: bigint[], previous?: GraphRowGeometry) => new GraphRowGeometry([{ ...fields[0],
      value: { kind: 'array', value: values.map(value => ({ kind: 'int', value })) } }], new Set(['links']), new Set(), true, previous)
    const initial = build([1n, 2n, 3n]), key = initial.rows[0].key
    const changed = build([1n, 20n, 3n], initial)
    expect(changed.collectionEpoch(key)).toBe(initial.collectionEpoch(key))
    const appended = build([1n, 20n, 3n, 4n], changed)
    expect(appended.collectionEpoch(key)).toBe(changed.collectionEpoch(key))
    const removed = build([20n, 3n, 4n], appended)
    expect(removed.collectionEpoch(key)).toBe(appended.collectionEpoch(key) + 1)
    const moved = build([3n, 20n, 4n], removed)
    expect(moved.collectionEpoch(key)).toBe(removed.collectionEpoch(key) + 1)
    expect(moved.resetsPath(moved.rows[1].key)).toBe(true)
    const replaced = build([20n, 4n, 5n], moved)
    expect(replaced.collectionEpoch(key)).toBe(moved.collectionEpoch(key) + 1)
  })
})
