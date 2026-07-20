import { describe, expect, it } from 'vitest'
import { moveDimensionCell, organizeDimensionRows } from './DimensionTableView'
import type { DimensionFileRow } from '../api'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'

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
    coordinate: { actual_type: 'Item', key },
    owner_file_path: owner,
    default_value: { kind: 'string', value: key },
    values: {},
  })

  it('projects owner record groups into managed dimension rows', () => {
    const groups: EditorProjectSettings['record_groups'] = {
      'data/items.cfd': {
        Item: [{
          id: 'g1',
          name: 'Items',
          color: 'blue',
          records: [
            { actual_type: 'Item', key: 'a' },
            { actual_type: 'Item', key: 'b' },
          ],
        }],
      },
    }
    const items = organizeDimensionRows([row('a'), row('b'), row('c')], groups, new Set())

    expect(items.map(item => item.kind)).toEqual(['group', 'row', 'row', 'ungrouped', 'row'])
    expect(items[0].kind === 'group' && items[0].view.group.color).toBe('blue')
    expect(items[3].kind === 'ungrouped' && items[3].rows[0].coordinate.key).toBe('c')
  })

  it('hides dimension members when their owner group is collapsed', () => {
    const groups: EditorProjectSettings['record_groups'] = {
      'data/items.cfd': {
        BaseItem: [{
          id: 'g1',
          name: 'Items',
          color: null,
          records: [
            { actual_type: 'Item', key: 'a' },
            { actual_type: 'Item', key: 'b' },
          ],
        }],
      },
    }
    const key = 'data/items.cfd\u001fBaseItem\u001fg1'
    const items = organizeDimensionRows([row('a'), row('b')], groups, new Set([key]))

    expect(items.map(item => item.kind)).toEqual(['group', 'ungrouped'])
  })
})
