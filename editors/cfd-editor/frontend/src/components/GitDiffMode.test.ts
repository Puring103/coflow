import { describe, expect, it } from 'vitest'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { buildDiffTree, changedTableColumns, GitDiffMode, hasProjectDiffChanges, projectTable, sourceLineDecorations } from './GitDiffMode'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { buildFileTreeGroups } from './FileTree'

const emptyDiff: ProjectDiff = { head_oid: 'abc', target_revision: 0, semantic_available: true, files: [], records: [], diagnostics: [] }

describe('Git Diff available views and tree', () => {
  it('hides semantic views for a selected source-only file even when another file has records', () => {
    const diff: ProjectDiff = { ...emptyDiff, files: [{ path: 'schema.cft', change: 'modified', before: '', after: '', patch: '' }], records: [{ coordinate: { actual_type: 'Item', key: 'one' }, change: 'added', after: { file_path: 'data/items.cfd', values: [] }, fields: [] }] }
    const html = renderToStaticMarkup(createElement(GitDiffMode, { diff, sessionId: 1, loading: false, error: null, selection: { filePath: 'schema.cft', coordinate: null }, onSelectionChange() {}, onRefresh() {} }))
    expect(html).not.toContain('>记录</button>')
    expect(html).not.toContain('>表格</button>')
    expect(html).toContain('aria-selected="true"')
    expect(html).toContain('源码</button>')
  })

  it('restores deleted paths and preserves the main tree dimension grouping', () => {
    const tree = buildDiffTree({ ...emptyDiff, files: ['schema/old.cft', 'data/nested/old.cfd', 'generated/lang/old.cfd'].map(path => ({ path, change: 'deleted', before: '', patch: '' })) }, [{ name: '本地化', path: 'generated/lang', is_dir: true, in_sources: false, in_schema: false, in_data: false, first_source_descendant: null, children: [] }])
    const groups = buildFileTreeGroups(tree, [{ name: 'language', display_name: '本地化', out_dir: 'generated/lang', variants: [], fields: [] }])
    expect(groups[0].nodes[0].children[0].path).toBe('schema/old.cft')
    expect(groups[1].nodes[0].children[0].children[0].path).toBe('data/nested/old.cfd')
    expect(groups[2].nodes[0].path).toBe('generated/lang/old.cfd')
  })
})

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

  it('filters unchanged columns and keeps nested changes and all added or deleted fields', () => {
    const values = ['stable', 'nested', 'addedOnly'].map(path => ({ path, value: { kind: 'int' as const, value: 1n } }))
    const diff: ProjectDiff = { ...emptyDiff, records: [{ coordinate: { actual_type: 'Item', key: 'one' }, change: 'modified', before: { file_path: 'data.cfd', values }, after: { file_path: 'data.cfd', values }, fields: [{ path: 'nested.value', change: 'modified' }] }] }
    expect(changedTableColumns(projectTable(diff, 'data.cfd', 'Item'))).toEqual(['nested'])
    for (const change of ['added', 'deleted'] as const) {
      diff.records.push({ coordinate: { actual_type: 'Item', key: change }, change, [change === 'added' ? 'after' : 'before']: { file_path: 'data.cfd', values: [values[2]] }, fields: [] })
      expect(changedTableColumns(projectTable(diff, 'data.cfd', 'Item'))).toEqual(['nested', 'addedOnly'])
    }
  })
})
