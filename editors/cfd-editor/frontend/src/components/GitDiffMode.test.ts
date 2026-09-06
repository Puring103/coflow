import { describe, expect, it } from 'vitest'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { hasProjectDiffChanges, projectTable, sourceLineDecorations } from './GitDiffMode'

describe('Git Diff result state', () => {
  it('only reports equality after receiving an empty comparison result', () => {
    const empty: ProjectDiff = {
      head_oid: 'abc',
      target_revision: 0,
      semantic_available: true,
      files: [],
      records: [],
      diagnostics: [],
    }

    expect(hasProjectDiffChanges(empty)).toBe(false)
    expect(hasProjectDiffChanges({
      ...empty,
      files: [{ path: 'data/items.cfd', change: 'modified', before: 'old', after: 'new', patch: '@@ -1 +1 @@\n-old\n+new' }],
    })).toBe(true)
  })
})

describe('Git Diff source projection', () => {
  it('classifies replacements separately from additions and deletions', () => {
    const result = sourceLineDecorations([
      '@@ -2,4 +2,4 @@',
      ' same',
      '-old',
      '+new',
      ' same',
      '-removed',
      ' same',
      '+added',
    ].join('\n'))

    expect(result.before).toEqual([
      { line: 3, className: 'cm-diff-modified' },
      { line: 5, className: 'cm-diff-deleted' },
    ])
    expect(result.after).toEqual([
      { line: 3, className: 'cm-diff-modified' },
      { line: 6, className: 'cm-diff-added' },
    ])
  })
})

describe('Git Diff table projection', () => {
  it('projects a modified record into adjacent HEAD and current rows', () => {
    const diff: ProjectDiff = {
      head_oid: 'abc',
      target_revision: 4,
      semantic_available: true,
      files: [],
      diagnostics: [],
      records: [{
        coordinate: { actual_type: 'Quest', key: 'advanced' },
        change: 'modified',
        before: { file_path: 'data/quests.cfd', values: [{ path: 'level', value: { kind: 'int', value: 1n } }] },
        after: { file_path: 'data/quests.cfd', values: [{ path: 'level', value: { kind: 'int', value: 2n } }] },
        fields: [{ path: 'level', change: 'modified', before: { kind: 'int', value: 1n }, after: { kind: 'int', value: 2n } }],
      }],
    }

    const projected = projectTable(diff, 'data/quests.cfd', 'Quest')

    expect(projected.data.records).toHaveLength(2)
    expect(projected.data.records.map(row => projected.presentations.get(row)?.version)).toEqual(['HEAD', '当前'])
    expect(new Set(projected.data.records.map(row => projected.presentations.get(row)?.id)).size).toBe(2)
    expect(projected.presentations.get(projected.data.records[0])?.changedFields).toEqual(new Set(['level']))
  })
})
