import { describe, expect, it } from 'vitest'
import { dimensionRowId, moveDimensionCell, organizeDimensionRows } from './DimensionTableView'
import type { DimensionFileRow } from '../../../api'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'

describe('moveDimensionCell', () => {
  it('moves in all four directions', () => {
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowLeft', 3, 4)).toEqual({ row: 1, column: 0 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowRight', 3, 4)).toEqual({ row: 1, column: 2 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowUp', 3, 4)).toEqual({ row: 0, column: 1 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowDown', 3, 4)).toEqual({ row: 2, column: 1 })
  })

  it('stays inside table boundaries', () => {
    expect(moveDimensionCell({ row: 0, column: 0 }, 'ArrowLeft', 2, 3)).toEqual({ row: 0, column: 0 })
    expect(moveDimensionCell({ row: 0, column: 0 }, 'ArrowUp', 2, 3)).toEqual({ row: 0, column: 0 })
    expect(moveDimensionCell({ row: 1, column: 2 }, 'ArrowRight', 2, 3)).toEqual({ row: 1, column: 2 })
    expect(moveDimensionCell({ row: 1, column: 2 }, 'ArrowDown', 2, 3)).toEqual({ row: 1, column: 2 })
  })
})

describe('organizeDimensionRows', () => {
  const row = (key: string, owner = 'data/items.cfd'): DimensionFileRow => ({
    coordinate: { actual_type: 'Item', id: key},
    field: 'name',
    owner_file_path: owner,
    sources: {},
    default_value: { kind: 'string', value: key },
    values: {},
  })

  it('distinguishes fields that share a singleton record coordinate', () => {
    expect(dimensionRowId(row('UiText'))).not.toBe(dimensionRowId({
      ...row('UiText'),
      field: 'farewell',
    }))
  })

  it('preserves first group priority, owner isolation and multiple fields', () => {
    const first = { id: 'first', name: 'First', color: null, records: [row('a').coordinate] }
    const second = { ...first, id: 'second' }
    const groups = { 'data/items.cfd': { Base: [first, second], Item: [second] } }
    const items = organizeDimensionRows([row('a'), { ...row('a'), field: 'description' }, row('a', 'other.cfd')], groups, new Set())
    expect(items.filter(item => item.kind === 'group').map(item => item.view.group.id)).toEqual(['first'])
    expect(items.filter(item => item.kind === 'row').map(item => item.row.field)).toEqual(['name', 'description', 'name'])
    expect(items.find(item => item.kind === 'ungrouped')?.rows.map(item => item.owner_file_path)).toEqual(['other.cfd'])
  })

  it('projects owner record groups into managed dimension rows', () => {
    const groups: EditorProjectSettings['record_groups'] = {
      'data/items.cfd': {
        Item: [{
          id: 'g1',
          name: 'Items',
          color: 'blue',
          records: [
            { actual_type: 'Item', id: 'a' },
            { actual_type: 'Item', id: 'b' },
          ],
        }],
      },
    }
    const items = organizeDimensionRows([row('a'), row('b'), row('c')], groups, new Set())

    expect(items.map(item => item.kind)).toEqual(['group', 'row', 'row', 'ungrouped', 'row'])
    expect(items[0].kind === 'group' && items[0].view.group.color).toBe('blue')
    expect(items[3].kind === 'ungrouped' && items[3].rows[0].coordinate.id).toBe('c')
  })

  it('hides dimension members when their owner group is collapsed', () => {
    const groups: EditorProjectSettings['record_groups'] = {
      'data/items.cfd': {
        BaseItem: [{
          id: 'g1',
          name: 'Items',
          color: null,
          records: [
            { actual_type: 'Item', id: 'a' },
            { actual_type: 'Item', id: 'b' },
          ],
        }],
      },
    }
    const key = 'data/items.cfd\u001fBaseItem\u001fg1'
    const items = organizeDimensionRows([row('a'), row('b')], groups, new Set([key]))

    expect(items.map(item => item.kind)).toEqual(['group', 'ungrouped'])
  })
})
