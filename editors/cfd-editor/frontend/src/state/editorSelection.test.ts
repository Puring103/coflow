import { describe, expect, it } from 'vitest'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { fieldPathField } from '../wire'
import {
  recordSelection,
  recordSelectionCoordinates,
  rebindSelection,
  removeSelection,
  selectionMatchesRecord,
  selectionMatchesValue,
  valueSelection,
  updateRecordSelection,
  updateValueSelection,
  valueSelectionCells,
} from './editorSelection'

const coordinate: RecordCoordinate = { actual_type: 'Npc', key: 'guard' }

describe('editor selection', () => {
  it('keeps record selection distinct from a value in that record', () => {
    const selection = recordSelection('data/npc.cfd', coordinate)

    expect(selectionMatchesRecord(selection, 'data/npc.cfd', coordinate)).toBe(true)
    expect(selectionMatchesValue(
      selection,
      'data/npc.cfd',
      coordinate,
      [fieldPathField('name')],
    )).toBe(false)
  })

  it('matches a selected value by file, coordinate, and complete field path', () => {
    const path = [fieldPathField('stats'), fieldPathField('health')]
    const selection = valueSelection('data/npc.cfd', coordinate, path)

    expect(selectionMatchesValue(selection, 'data/npc.cfd', coordinate, path)).toBe(true)
    expect(selectionMatchesValue(
      selection,
      'data/npc.cfd',
      coordinate,
      [fieldPathField('stats'), fieldPathField('mana')],
    )).toBe(false)
    expect(selectionMatchesRecord(selection, 'data/npc.cfd', coordinate)).toBe(false)
  })

  it('rebinds a selected value only when file and coordinate both match', () => {
    const selection = valueSelection(
      'data/npc.cfd',
      coordinate,
      [fieldPathField('name')],
    )
    const renamed = { ...coordinate, key: 'captain' }

    expect(rebindSelection(selection, 'data/items.cfd', coordinate, renamed)).toBe(selection)
    expect(rebindSelection(selection, 'data/npc.cfd', coordinate, renamed)).toEqual({
      ...selection,
      coordinate: renamed,
      rangeAnchor: { ...selection.rangeAnchor, coordinate: renamed },
    })
  })

  it('removes record and value selections only after the owning record matches', () => {
    const record = recordSelection('data/npc.cfd', coordinate)
    const value = valueSelection('data/npc.cfd', coordinate, [fieldPathField('name')])

    expect(removeSelection(record, 'data/items.cfd', coordinate)).toBe(record)
    expect(removeSelection(value, 'data/npc.cfd', coordinate)).toBeNull()
    expect(removeSelection(record, 'data/npc.cfd', coordinate)).toBeNull()
  })

  it('toggles records and selects visible ranges from a stable anchor', () => {
    const rows: RecordCoordinate[] = [
      coordinate,
      { actual_type: 'Npc', key: 'merchant' },
      { actual_type: 'Npc', key: 'captain' },
    ]
    const first = recordSelection('data/npc.cfd', rows[0])
    const toggled = updateRecordSelection(first, 'data/npc.cfd', rows[2], rows, 'toggle')
    const ranged = updateRecordSelection(toggled, 'data/npc.cfd', rows[1], rows, 'range')

    expect(recordSelectionCoordinates(toggled).map(item => item.key)).toEqual(['guard', 'captain'])
    expect(recordSelectionCoordinates(ranged).map(item => item.key)).toEqual(['guard', 'merchant'])
    expect(ranged?.coordinate).toEqual(rows[1])
  })

  it('removes one coordinate from a multi-record selection', () => {
    const second = { actual_type: 'Npc', key: 'merchant' }
    const selection = updateRecordSelection(
      recordSelection('data/npc.cfd', coordinate),
      'data/npc.cfd',
      second,
      [coordinate, second],
      'toggle',
    )

    expect(removeSelection(selection, 'data/npc.cfd', coordinate)).toEqual(
      recordSelection('data/npc.cfd', second),
    )
  })

  it('enumerates a rectangular value range in visible row-major order', () => {
    const second = { actual_type: 'Npc', key: 'merchant' }
    const rows = [coordinate, second]
    const start = valueSelection('data/npc.cfd', coordinate, [fieldPathField('name')])
    const range = updateValueSelection(
      start,
      'data/npc.cfd',
      second,
      [fieldPathField('hp')],
      'range',
    )

    expect(valueSelectionCells(range, rows, ['name', 'hp']).map(cell => (
      `${cell.coordinate.key}:${cell.fieldPath[0].value}`
    ))).toEqual(['guard:name', 'guard:hp', 'merchant:name', 'merchant:hp'])
    expect(selectionMatchesValue(
      range,
      'data/npc.cfd',
      coordinate,
      [fieldPathField('hp')],
      rows,
      ['name', 'hp'],
    )).toBe(true)
  })

  it('keeps the original anchor across consecutive range updates', () => {
    const rows = [
      coordinate,
      { actual_type: 'Npc', key: 'merchant' },
      { actual_type: 'Npc', key: 'captain' },
    ]
    const start = valueSelection('data/npc.cfd', rows[0], [fieldPathField('name')])
    const first = updateValueSelection(start, start.filePath, rows[1], [fieldPathField('hp')], 'range')
    const second = updateValueSelection(first, start.filePath, rows[2], [fieldPathField('hp')], 'range')

    expect(valueSelectionCells(second, rows, ['name', 'hp'])).toHaveLength(6)
    expect(second.rangeAnchor.coordinate).toEqual(rows[0])
  })
})
