import { describe, expect, it } from 'vitest'
import { MOCK_FILE_RECORDS } from '../mock'
import type { PluginSearchHit } from '../plugins'
import { groupSearchHits, searchMockRecords } from './projectSearch'

describe('built-in project search plugin', () => {
  it('groups hits by file and actual type without changing hit order', () => {
    const hits: PluginSearchHit[] = [
      hit('data/items.cfd', 'Item', 'potion'),
      hit('data/items.cfd', 'Weapon', 'sword'),
      hit('data/npcs.cfd', 'Npc', 'smith'),
    ]

    const groups = groupSearchHits(hits)

    expect(groups.map(group => [group.filePath, group.hits.length])).toEqual([
      ['data/items.cfd', 2],
      ['data/npcs.cfd', 1],
    ])
    expect(groups[0].types.map(group => group.actualType)).toEqual(['Item', 'Weapon'])
  })

  it('searches unopened mock files by key and nested field value', () => {
    const keyResults = searchMockRecords(MOCK_FILE_RECORDS, 3, 1, 'npc_002', 'key', 200)
    expect(keyResults.data.hits[0].filePath).toBe('data/npc.cfd')

    const textResults = searchMockRecords(MOCK_FILE_RECORDS, 3, 1, '铁匠', 'full_text', 200)
    expect(textResults.data.hits[0]).toMatchObject({
      filePath: 'data/npc.cfd',
      fieldPath: 'name',
      preview: 'name: 铁匠',
    })
    expect(textResults).toMatchObject({ sessionId: 3, revision: 1 })
  })

  it('reports truncation only when another match exists', () => {
    const results = searchMockRecords(MOCK_FILE_RECORDS, 3, 1, 'item_', 'key', 1)
    expect(results.data.hits).toHaveLength(1)
    expect(results.data.truncated).toBe(true)
  })
})

function hit(filePath: string, actualType: string, key: string): PluginSearchHit {
  return {
    filePath,
    coordinate: { actual_type: actualType, key },
    fieldPath: null,
    preview: null,
  }
}
