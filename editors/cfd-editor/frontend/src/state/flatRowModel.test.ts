import { createTable, getCoreRowModel, getFilteredRowModel, getSortedRowModel, type ColumnDef, type Table } from '@tanstack/react-table'
import { describe, expect, it } from 'vitest'
import { getSharedFlatRowModel } from './flatRowModel'

type Item = { id: string; value: number }
const columns: ColumnDef<Item>[] = [{ id: 'value', accessorFn: row => row.value }]
function make(shared: boolean, data: Item[]) {
  return createTable<Item>({
    data, columns, getRowId: row => row.id, state: { sorting: [], columnFilters: [], globalFilter: undefined },
    onStateChange: () => {}, renderFallbackValue: null,
    getCoreRowModel: shared ? getSharedFlatRowModel() : getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(), getSortedRowModel: getSortedRowModel(),
  })
}
function snapshot(table: Table<Item>) {
  return table.getRowModel().rows.map(row => ({ id: row.id, index: row.index, original: row.original, value: row.getValue('value'), unique: row.getUniqueValues('value'), cells: row.getAllCells().map(cell => cell.getValue()) }))
}

describe('shared flat row model', () => {
  it('matches standard rows through edits, sorting, filtering, membership, and accessor changes', () => {
    const original = [{ id: 'a', value: 3 }, { id: 'b', value: 1 }, { id: 'c', value: 2 }]
    const shared = make(true, original), standard = make(false, original)
    expect(snapshot(shared)).toEqual(snapshot(standard))
    const old = shared.getCoreRowModel().rows.slice()
    const edited = [original[0], { ...original[1], value: 9 }, original[2]]
    for (const table of [shared, standard]) table.setOptions(options => ({ ...options, data: edited }))
    expect(snapshot(shared)).toEqual(snapshot(standard))
    expect(shared.getCoreRowModel().rows[0]).toBe(old[0])
    expect(shared.getCoreRowModel().rows[1]).not.toBe(old[1])
    expect(old[1].original.value).toBe(1)
    for (const data of [edited, [...edited].reverse(), [edited[2], { id: 'd', value: 4 }], []]) {
      for (const desc of [false, true]) {
        for (const filtered of [false, true]) {
          for (const table of [shared, standard]) table.setOptions(options => ({ ...options, data, state: { ...options.state, sorting: [{ id: 'value', desc }], columnFilters: filtered ? [{ id: 'value', value: [2, 5] }] : [] } }))
          expect(snapshot(shared)).toEqual(snapshot(standard))
        }
      }
    }
    for (const multiplier of [1, -2]) {
      const nextColumns: ColumnDef<Item>[] = [{ id: 'value', accessorFn: row => row.value * multiplier }]
      for (const table of [shared, standard]) table.setOptions(options => ({ ...options, data: [...original], columns: nextColumns, state: { sorting: [], columnFilters: [] } }))
      expect(snapshot(shared)).toEqual(snapshot(standard))
    }
  })
})
