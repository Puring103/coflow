import { describe, expect, it } from 'vitest'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  moveRecordOntoRecord,
  moveRecordToGroup,
  organizeRecordRows,
  removeRecordFromGroups,
  renameRecordGroup,
  replaceGroupedCoordinate,
} from './manualRecordGroups'

const coordinate = (key: string): RecordCoordinate => ({ actual_type: 'Item', key })
const row = (key: string): RecordRow => ({
  coordinate: coordinate(key),
  display_path: key,
  fields: [],
  field_index: {},
  field_summaries: {},
  field_diagnostics: [],
  diagnostic_severity: null,
})
const group = (id: string, ...keys: string[]): EditorRecordGroup => ({
  id,
  name: id,
  records: keys.map(coordinate),
})

describe('manual record groups', () => {
  it('creates a group by dropping one ungrouped record onto another', () => {
    expect(moveRecordOntoRecord([], coordinate('b'), coordinate('a'), 'g1', '新分组')).toEqual([
      { id: 'g1', name: '新分组', records: [coordinate('a'), coordinate('b')] },
    ])
  })

  it('joins the target group when dropped onto one of its records', () => {
    expect(moveRecordOntoRecord(
      [group('g1', 'a', 'b')],
      coordinate('c'),
      coordinate('b'),
      'unused',
      'unused',
    )).toEqual([group('g1', 'a', 'b', 'c')])
  })

  it('moves records between groups and dissolves a one-record remainder', () => {
    const result = moveRecordToGroup(
      [group('source', 'a', 'b'), group('target', 'c', 'd')],
      coordinate('a'),
      'target',
    )
    expect(result).toEqual([group('target', 'c', 'd', 'a')])
  })

  it('removes a record and dissolves a group that no longer has two members', () => {
    expect(removeRecordFromGroups([group('g1', 'a', 'b')], coordinate('a'))).toEqual([])
  })

  it('organizes valid members and leaves stale or duplicate members ungrouped', () => {
    const result = organizeRecordRows(
      [row('a'), row('b'), row('c')],
      [group('g1', 'a', 'b'), group('g2', 'b', 'missing')],
    )
    expect(result.groups.map(view => view.records.map(record => record.coordinate.key))).toEqual([['a', 'b']])
    expect(result.ungrouped.map(record => record.coordinate.key)).toEqual(['c'])
  })

  it('renames groups and follows record key changes', () => {
    const renamed = renameRecordGroup([group('g1', 'a', 'b')], 'g1', ' Potions ')
    expect(renamed[0].name).toBe('Potions')
    expect(replaceGroupedCoordinate(renamed, coordinate('a'), coordinate('renamed'))[0].records[0])
      .toEqual(coordinate('renamed'))
  })
})
