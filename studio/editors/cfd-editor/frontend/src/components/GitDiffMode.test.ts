import { describe, expect, it } from 'vitest'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { ancestorPathKeys, buildDiffTree, changedTableColumns, GitDiffMode, GitDiffSidebar, hasProjectDiffChanges, projectTable, sourceLineDecorations } from './GitDiffMode'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { buildFileTreeGroups } from './FileTree'

const emptyDiff: ProjectDiff = { head_oid: 'abc', target_revision: 0, semantic_available: true, files: [], records: [], diagnostics: [] }
const fileTypes = { 'data/items.cfd': [{ name: 'Item', display_name: 'Items', record_count: 1, is_singleton: false, dimension_fields: {} }] }

describe('Git Diff available views and tree', () => {
  it('hides semantic views for a selected source-only file even when another file has records', () => {
    const diff: ProjectDiff = { ...emptyDiff, files: [{ path: 'schema.cft', change: 'modified', before: '', after: '', patch: '' }], records: [{ coordinate: { actual_type: 'Item', key: 'one' }, change: 'added', after: { file_path: 'data/items.cfd', values: [] }, fields: [] }] }
    const html = renderToStaticMarkup(createElement(GitDiffMode, { diff, sessionId: 1, fileTypes, loading: false, error: null, selection: { filePath: 'schema.cft', typeName: null, coordinate: null }, onSelectionChange() {}, onRefresh() {} }))
    expect(html).not.toContain('>记录</button>')
    expect(html).not.toContain('>表格</button>')
    expect(html).toContain('aria-selected="true"')
    expect(html).toContain('源码</button>')
  })

  it('shows record and source views for singleton types without a table view', () => {
    const diff: ProjectDiff = { ...emptyDiff, records: [{ coordinate: { actual_type: 'Settings', key: 'only' }, change: 'modified', before: { file_path: 'data/settings.cfd', values: [] }, after: { file_path: 'data/settings.cfd', values: [] }, fields: [] }] }
    const singletonTypes = { 'data/settings.cfd': [{ name: 'Settings', display_name: 'Settings', record_count: 1, is_singleton: true, dimension_fields: {} }] }
    const html = renderToStaticMarkup(createElement(GitDiffMode, { diff, sessionId: 1, fileTypes: singletonTypes, loading: false, error: null, selection: { filePath: 'data/settings.cfd', typeName: 'Settings', coordinate: null }, onSelectionChange() {}, onRefresh() {} }))
    expect(html).toContain('>记录</button>')
    expect(html).not.toContain('>表格</button>')
    expect(html).toContain('>源码</button>')
  })

  it('restores deleted paths and preserves the main tree dimension grouping', () => {
    const tree = buildDiffTree({ ...emptyDiff, files: ['schema/old.cft', 'data/nested/old.cfd', 'generated/lang/old.cfd'].map(path => ({ path, change: 'deleted', before: '', patch: '' })) }, [{ name: '本地化', path: 'generated/lang', is_dir: true, in_sources: false, in_schema: false, in_data: false, first_source_descendant: null, children: [] }])
    const groups = buildFileTreeGroups(tree, [{ name: 'language', display_name: '本地化', variants: [], fields: [] }])
    expect(groups[0].nodes[0].children[0].path).toBe('schema/old.cft')
    expect(JSON.stringify(groups[1].nodes)).toContain('data/nested/old.cfd')
    expect(JSON.stringify(groups[1].nodes)).toContain('generated/lang/old.cfd')
    expect(groups[2].dimensionNodes).toEqual([])
  })

  it('keeps only changed files in the file tree', () => {
    const tree = buildDiffTree(
      { ...emptyDiff, files: [{ path: 'data/changed.cfd', change: 'modified', before: 'a', after: 'b', patch: '' }] },
      [
        { name: 'changed.cfd', path: 'data/changed.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: null, children: [] },
        { name: 'same.cfd', path: 'data/same.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: null, children: [] },
      ],
    )
    expect(tree.map(node => node.path)).toEqual(['data/changed.cfd'])
  })

  it('renders multi-type files without changed records as a selectable single row', () => {
    // 本地化生成文件等多类型但无语义记录变化的文件，必须可点击选中。
    const diff: ProjectDiff = {
      ...emptyDiff,
      files: [{ path: 'dimensions/language/Item_name.cfd', change: 'modified', before: 'a', after: 'b', patch: '' }],
      records: [],
    }
    const nodes = [
      { name: 'Item_name.cfd', path: 'dimensions/language/Item_name.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: null, children: [] },
    ]
    const multiTypes = {
      'dimensions/language/Item_name.cfd': [
        { name: 'Item', display_name: 'Items', record_count: 0, is_singleton: false, dimension_fields: {} },
        { name: 'Weapon', display_name: 'Weapons', record_count: 0, is_singleton: false, dimension_fields: {} },
      ],
    }
    const html = renderToStaticMarkup(createElement(GitDiffSidebar, {
      diff,
      nodes,
      dimensions: [{ name: 'language', display_name: '本地化', variants: [], fields: [] }],
      fileTypes: multiTypes,
      loading: false,
      error: null,
      selection: { filePath: null, typeName: null, coordinate: null },
      onSelectionChange() {},
      onRefresh() {},
    }))
    expect(html).toContain('data-file-path="dimensions/language/Item_name.cfd"')
    expect(html).not.toContain('tree-file-parent')
  })

  it('renders a single row without a dropdown when only one type changed', () => {
    // 多类型文件中仅一个类型有变化时，直接单行选中该类型，无需展开。
    const diff: ProjectDiff = {
      ...emptyDiff,
      files: [],
      records: [{
        coordinate: { actual_type: 'Item', key: 'one' },
        change: 'modified',
        before: { file_path: 'data/mixed.cfd', values: [] },
        after: { file_path: 'data/mixed.cfd', values: [] },
        fields: [],
      }],
    }
    const nodes = [
      { name: 'mixed.cfd', path: 'data/mixed.cfd', is_dir: false, in_sources: true, in_schema: false, in_data: true, first_source_descendant: null, children: [] },
    ]
    const mixedTypes = {
      'data/mixed.cfd': [
        { name: 'Item', display_name: 'Items', record_count: 1, is_singleton: false, dimension_fields: {} },
        { name: 'Weapon', display_name: 'Weapons', record_count: 0, is_singleton: false, dimension_fields: {} },
      ],
    }
    const html = renderToStaticMarkup(createElement(GitDiffSidebar, {
      diff,
      nodes,
      dimensions: [],
      fileTypes: mixedTypes,
      loading: false,
      error: null,
      selection: { filePath: null, typeName: null, coordinate: null },
      onSelectionChange() {},
      onRefresh() {},
    }))
    expect(html).not.toContain('tree-file-parent')
    expect(html).toContain('data-type-name="Item"')
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
  it('projects a modified record into before and after rows in order', () => {
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
    expect(projected.data.records.map(row => projected.presentations.get(row)?.groupStart)).toEqual([true, undefined])
    expect(new Set(projected.data.records.map(row => projected.presentations.get(row)?.id)).size).toBe(2)
    expect(projected.presentations.get(projected.data.records[0])?.changedFields).toEqual(new Set(['level']))
    expect(projected.presentations.get(projected.data.records[1])?.changedFields).toEqual(new Set(['level']))
  })

  it('expands ancestor paths of changed fields for record auto-expansion', () => {
    expect(ancestorPathKeys(new Set(['level']))).toEqual(new Set())
    expect(ancestorPathKeys(new Set(['nested.value', 'Desc[language=en]', 'a.b[0].c']))).toEqual(
      new Set(['nested', 'Desc', 'a', 'a.b', 'a.b[0]']),
    )
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
