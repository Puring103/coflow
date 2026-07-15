import { describe, expect, it } from 'vitest'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { fieldPathField } from '../wire'
import {
  recordSelection,
  selectionMatchesRecord,
  selectionMatchesValue,
  valueSelection,
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
})
