import { describe, expect, it } from 'vitest'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { coordinateId, fieldPathField } from '../wire'
import {
  cellAnchorsIdentity,
  iterateSelectedCells,
  selectedCellCount,
  selectedCellsIdentity,
  editorSelectionIdentity,
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
  valueSelectionRange,
} from './editorSelection'

const coordinate: RecordCoordinate = { actual_type: 'Npc', id: 'guard' }

describe('editor selection', () => {
  it('matches indexed ranges to scans after sorting, filtering, and missing fields', () => {
    const allRows = Array.from({ length: 1000 }, (_, index) => ({ actual_type: 'Npc', id: `npc${index}` }))
    const selection = updateValueSelection(valueSelection('data.cfd', allRows[900], [fieldPathField('name')]), 'data.cfd', allRows[200], [fieldPathField('value')], 'range')
    for (const rows of [allRows, [...allRows].reverse(), allRows.slice(300)]) {
      for (const columns of [['name', 'value'], ['value', 'name'], ['name']]) {
        const indexes = {
          rows: new Map(rows.map((coordinate, index) => [coordinateId(coordinate), index])),
          columns: new Map(columns.map((column, index) => [column, index])),
        }
        expect(valueSelectionRange(selection, rows, columns, indexes)).toEqual(valueSelectionRange(selection, rows, columns))
      }
    }
  })

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
    const renamed = { ...coordinate, id: 'captain' }

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
      { actual_type: 'Npc', id: 'merchant' },
      { actual_type: 'Npc', id: 'captain' },
    ]
    const first = recordSelection('data/npc.cfd', rows[0])
    const toggled = updateRecordSelection(first, 'data/npc.cfd', rows[2], rows, 'toggle')
    const ranged = updateRecordSelection(toggled, 'data/npc.cfd', rows[1], rows, 'range')

    expect(recordSelectionCoordinates(toggled).map(item => item.id)).toEqual(['guard', 'captain'])
    expect(recordSelectionCoordinates(ranged).map(item => item.id)).toEqual(['guard', 'merchant'])
    expect(ranged?.coordinate).toEqual(rows[1])
  })

  it('removes one coordinate from a multi-record selection', () => {
    const second = { actual_type: 'Npc', id: 'merchant' }
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
    const second = { actual_type: 'Npc', id: 'merchant' }
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
      `${cell.coordinate.id}:${cell.fieldPath[0].value}`
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
      { actual_type: 'Npc', id: 'merchant' },
      { actual_type: 'Npc', id: 'captain' },
    ]
    const start = valueSelection('data/npc.cfd', rows[0], [fieldPathField('name')])
    const first = updateValueSelection(start, start.filePath, rows[1], [fieldPathField('hp')], 'range')
    const second = updateValueSelection(first, start.filePath, rows[2], [fieldPathField('hp')], 'range')

    expect(valueSelectionCells(second, rows, ['name', 'hp'])).toHaveLength(6)
    expect(second.rangeAnchor.coordinate).toEqual(rows[0])
  })

  it('changes the edit identity when the record or field target changes', () => {
    const name = valueSelection('data/npc.cfd', coordinate, [fieldPathField('name')])
    const health = valueSelection('data/npc.cfd', coordinate, [fieldPathField('health')])
    const merchant = valueSelection(
      'data/npc.cfd',
      { actual_type: 'Npc', id: 'merchant' },
      [fieldPathField('name')],
    )

    expect(editorSelectionIdentity(name)).not.toBe(editorSelectionIdentity(health))
    expect(editorSelectionIdentity(name)).not.toBe(editorSelectionIdentity(merchant))
  })

  it('keeps files and field paths with separators distinct', () => {
    const a = valueSelection('data:a', coordinate, [fieldPathField('x:y')])
    const b = valueSelection('data', { actual_type: 'a:Npc', id: 'guard' }, [fieldPathField('x:y')])
    expect(editorSelectionIdentity(a)).not.toBe(editorSelectionIdentity(b))
    expect(cellAnchorsIdentity('data:a', [{ coordinate, fieldPath: [fieldPathField('x:y')] }]))
      .not.toBe(cellAnchorsIdentity('data', [{ coordinate, fieldPath: [fieldPathField('a:x:y')] }]))
  })

  it('uses an order-independent identity for batch record and cell targets', () => {
    const merchant = { actual_type: 'Npc', id: 'merchant' }
    const first = updateRecordSelection(
      recordSelection('data/npc.cfd', coordinate),
      'data/npc.cfd',
      merchant,
      [coordinate, merchant],
      'toggle',
    )!
    const second = updateRecordSelection(
      recordSelection('data/npc.cfd', merchant),
      'data/npc.cfd',
      coordinate,
      [coordinate, merchant],
      'toggle',
    )!
    const name = [fieldPathField('name')]

    expect(editorSelectionIdentity(first)).toBe(editorSelectionIdentity(second))
    expect(cellAnchorsIdentity('data/npc.cfd', [
      { coordinate, fieldPath: name },
      { coordinate: merchant, fieldPath: name },
    ])).toBe(cellAnchorsIdentity('data/npc.cfd', [
      { coordinate: merchant, fieldPath: name },
      { coordinate, fieldPath: name },
    ]))
  })
})

describe('compact cell ranges', () => {
  it('iterates in view order and keeps a million-cell selection compact', () => {
    const rows = Array.from({ length: 10000 }, (_, id) => ({ actual_type: 'Item', id: String(id) }))
    const columns = Array.from({ length: 100 }, (_, id) => `f${id}`)
    const selected = { rows, columns, range: { rowStart: 0, rowEnd: 9999, columnStart: 0, columnEnd: 99 } }
    expect(selectedCellCount(selected)).toBe(1000000)
    const iterator = iterateSelectedCells(selected)
    expect(iterator.next().value).toEqual({ coordinate: rows[0], fieldPath: [fieldPathField('f0')] })
    expect(iterator.next().value).toEqual({ coordinate: rows[0], fieldPath: [fieldPathField('f1')] })
    expect(Array.from(iterateSelectedCells({ rows: [...rows].reverse(), columns, range: { rowStart: 1, rowEnd: 2, columnStart: 3, columnEnd: 3 } })).map(cell => cell.coordinate.id)).toEqual(['9998', '9997'])
  })
})

it('keeps range editor identity stable when the same rows and columns are reordered', () => {
  const rows = [{ actual_type: 'Item', id: 'a' }, { actual_type: 'Item', id: 'b' }]
  const columns = ['name', 'title']
  const range = { rowStart: 0, rowEnd: 1, columnStart: 0, columnEnd: 1 }
  expect(selectedCellsIdentity('a.cfd', { rows, columns, range }))
    .toBe(selectedCellsIdentity('a.cfd', { rows: [...rows].reverse(), columns: [...columns].reverse(), range }))
  expect(selectedCellsIdentity('a.cfd', { rows, columns, range }))
    .not.toBe(selectedCellsIdentity('b.cfd', { rows, columns, range }))
})
