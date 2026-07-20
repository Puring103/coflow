import { describe, expect, it } from 'vitest'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  createRecordGroup,
  moveRecordOntoRecord,
  moveRecordsOntoRecord,
  moveRecordsToGroup,
  moveRecordToGroup,
  organizeRecordRows,
  removeRecordFromGroups,
  removeRecordsFromGroups,
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
  color: null,
  records: keys.map(coordinate),
})

describe('manual record groups', () => {
  it('creates a new group from selected records even when they were grouped before', () => {
    expect(createRecordGroup(
      [group('old', 'a', 'b', 'c')],
      [coordinate('a'), coordinate('b')],
      'new',
      'Selected',
    )).toEqual([
      { id: 'new', name: 'Selected', color: null, records: [coordinate('a'), coordinate('b')] },
    ])
  })

  it('creates a group by dropping one ungrouped record onto another', () => {
    expect(moveRecordOntoRecord([], coordinate('b'), coordinate('a'), 'g1', '新分组')).toEqual([
      { id: 'g1', name: '新分组', color: null, records: [coordinate('a'), coordinate('b')] },
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

  it('moves multiple records in one operation and dissolves the source remainder', () => {
    expect(moveRecordsToGroup(
      [group('source', 'a', 'b', 'c'), group('target', 'd', 'e')],
      [coordinate('a'), coordinate('b')],
      'target',
    )).toEqual([group('target', 'd', 'e', 'a', 'b')])
  })

  it('creates one group when multiple records are dropped onto an ungrouped record', () => {
    expect(moveRecordsOntoRecord(
      [group('source', 'a', 'b', 'c')],
      [coordinate('a'), coordinate('b')],
      coordinate('d'),
      'new',
      'New',
    )).toEqual([{ id: 'new', name: 'New', color: null, records: [coordinate('d'), coordinate('a'), coordinate('b')] }])
  })

  it('removes multiple records atomically', () => {
    expect(removeRecordsFromGroups(
      [group('source', 'a', 'b', 'c'), group('other', 'd', 'e')],
      [coordinate('a'), coordinate('b')],
    )).toEqual([group('other', 'd', 'e')])
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
