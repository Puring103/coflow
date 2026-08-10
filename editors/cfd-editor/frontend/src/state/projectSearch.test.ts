import { describe, expect, it } from 'vitest'
import type { ProjectSearchHit } from '../bindings/ProjectSearchHit'
import { MOCK_FILE_RECORDS } from '../mock'
import { groupProjectSearchHits, searchMockRecords } from './projectSearch'

describe('project search', () => {
  it('groups hits by file and actual type without changing hit order', () => {
    const hits: ProjectSearchHit[] = [
      hit('data/items.cfd', 'Item', 'potion'),
      hit('data/items.cfd', 'Weapon', 'sword'),
      hit('data/npcs.cfd', 'Npc', 'smith'),
    ]
    const groups = groupProjectSearchHits(hits)
    expect(groups.map(group => [group.filePath, group.hits.length])).toEqual([
      ['data/items.cfd', 2],
      ['data/npcs.cfd', 1],
    ])
    expect(groups[0].types.map(group => group.actualType)).toEqual(['Item', 'Weapon'])
  })

  it('searches unopened mock files by key and nested field value', () => {
    const keyResults = searchMockRecords(MOCK_FILE_RECORDS, 1, 'npc_002', 'key', 200)
    expect(keyResults.hits[0].file_path).toBe('data/npc.cfd')

    const textResults = searchMockRecords(MOCK_FILE_RECORDS, 1, '铁匠', 'full_text', 200)
    expect(textResults.hits[0]).toMatchObject({
      file_path: 'data/npc.cfd',
      field_path: 'name',
      preview: 'name: 铁匠',
    })
  })

  it('reports truncation only when another match exists', () => {
    const results = searchMockRecords(MOCK_FILE_RECORDS, 1, 'item_', 'key', 1)
    expect(results.hits).toHaveLength(1)
    expect(results.truncated).toBe(true)
  })
})

function hit(filePath: string, actualType: string, key: string): ProjectSearchHit {
  return {
    file_path: filePath,
    coordinate: { actual_type: actualType, key },
    field_path: null,
    preview: null,
  }
}
