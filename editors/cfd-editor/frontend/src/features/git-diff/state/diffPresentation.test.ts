import { describe, expect, it } from 'vitest'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import type { CfdValue } from '../../../bindings/CfdValue'
import type { ProjectRecordDiff } from '../../../bindings/ProjectRecordDiff'
import type { ProjectDiff } from '../../../bindings/ProjectDiff'
import { changedRecordFields, expandedDiffPaths } from './diffPresentation'
import { planDiffRowAlignment } from './diffRowAlignment'
import { GitDiffSidebar, projectTable, changedTableColumns } from '../components/GitDiffMode'
import { RecordView } from '../../record-editing/components/RecordView'
import { TableView } from '../../table/components/TableView'

const coordinate = { actual_type: 'Item', id: 'one' }
const value = { kind: 'array' as const, value: [{ kind: 'object' as const, value: {
  actual_type: 'Stats', fields: { hp: { kind: 'int' as const, value: 2n } },
} }] }
const record: ProjectRecordDiff = {
  coordinate, change: 'modified',
  before: { file_path: 'data.cfd', values: [{ path: 'stats', value }] },
  after: { file_path: 'data.cfd', values: [{ path: 'stats', value, display: {
    label: '属性', description: '角色属性列表', children: {
      '0': { label: null, description: null, children: {
        hp: { label: '生命值', description: '最大生命值', children: {} },
      } },
    },
  } }] },
  fields: [{ path: 'stats[0].hp', change: 'modified' }],
}
const diff: ProjectDiff = { head_oid: 'abc', target_revision: 1, semantic_available: true, files: [], diagnostics: [], records: [record] }

describe('shared diff presentation', () => {
  it('does not reuse another snapshot’s label and description in shared table headers', () => {
    const render = (label: string, description: string) => {
      const projected = projectTable({ ...diff, records: [{ coordinate, change: 'added', fields: [],
        after: { file_path: 'data.cfd', values: [{ path: 'title', value: { kind: 'string', value: 'value' },
          display: { label, description, children: {} } }] },
      }] }, 'data.cfd', 'Item')
      return renderToStaticMarkup(createElement(TableView, {
        data: projected.data, activeType: 'Item', readOnly: true, onOpenRecord() {},
        rowPresentation: row => projected.presentations.get(row),
      }))
    }
    expect(render('旧名称', '旧描述')).toContain('旧名称')
    const current = render('新名称', '新描述')
    expect(current).toContain('新名称')
    expect(current).toContain('新描述')
    expect(current).not.toContain('旧名称')
  })

  it('expands changed structures on both sides and renders endpoint labels and descriptions through RecordView', () => {
    const paths = expandedDiffPaths(record)
    expect(paths).toEqual(new Set(['stats', 'stats[0]']))
    const projected = projectTable(diff, 'data.cfd', 'Item')
    for (const row of projected.data.records) expect(projected.presentations.get(row)?.expandedPaths).toEqual(paths)
    const html = renderToStaticMarkup(createElement(RecordView, {
      data: { ...projected.data, records: [{ ...projected.data.records[1], coordinate: projected.presentations.get(projected.data.records[1])!.coordinate! }] }, coordinate,
      readOnly: true, hideRecordList: true, onOpenRecord() {}, expandedPaths: paths,
    }))
    expect(html).toContain('属性')
    expect(html).toContain('角色属性列表')
    expect(html).toContain('生命值')
    expect(html).toContain('最大生命值')
    expect(html).toContain('data-field-path="stats[0].hp"')
  })

  it('keeps dimension columns intact and out of base record projections', () => {
    const overlay: ProjectRecordDiff = {
      ...record,
      before: { file_path: 'lang/data.cfd', dimension: 'language', values: [{ path: 'name[language=zh]', value: { kind: 'string', value: '旧' } }] },
      after: { file_path: 'lang/data.cfd', dimension: 'language', values: [{ path: 'name[language=zh]', value: { kind: 'string', value: '新' } }] },
      fields: [{ path: 'name[language=zh]', change: 'modified' }],
    }
    const source = { ...diff, records: [record, overlay] }
    expect(changedRecordFields(overlay)).toEqual(new Set(['name[language=zh]']))
    expect(projectTable(source, 'lang/data.cfd', 'Item').data.records).toHaveLength(0)
    const dimension = projectTable(source, 'lang/data.cfd', 'Item', 'language')
    expect(dimension.data.records).toHaveLength(2)
    expect(changedTableColumns(dimension)).toEqual(['name[language=zh]'])
  })

  it('restores a deleted dimension file under its dimension with no record dropdown', () => {
    const deleted: ProjectRecordDiff = { coordinate, change: 'deleted', fields: [],
      before: { file_path: 'lang/deleted.cfd', dimension: 'language', values: [] } }
    const html = renderToStaticMarkup(createElement(GitDiffSidebar, {
      diff: { ...diff, records: [deleted] }, nodes: [],
      dimensions: [{ name: 'language', display_name: '本地化', variants: [], fields: [], dir: 'lang', files: [] }],
      fileTypes: {}, loading: false, error: null,
      selection: { filePath: 'lang/deleted.cfd', dimension: 'language', coordinate: null },
      onSelectionChange() {}, onRefresh() {},
    }))
    expect(html).toContain('<strong>本地化</strong>')
    expect(html).not.toContain('<strong>数据</strong>')
    expect(html).not.toContain('<strong>维度</strong>')
    expect(html).toContain('data-file-path="lang/deleted.cfd"')
    expect(html).toContain('aria-selected="true"')
    expect(html).not.toContain('tree-file-parent')
    expect(html).not.toContain('Item.one')
  })

  it('finds changed ancestors without parsing punctuation inside dictionary keys', () => {
    const field = 'name[language=zh]'
    const path = `${field}["a.b[0]"]`
    const nested: CfdValue = { kind: 'dict', value: [[{ kind: 'string', value: 'a.b[0]' }, value]] }
    const complex: ProjectRecordDiff = { coordinate, change: 'modified', fields: [{ path: `${path}[0].hp`, change: 'modified' }],
      after: { file_path: 'lang.cfd', dimension: 'language', values: [{ path: field, value: nested }] } }
    expect(expandedDiffPaths(complex)).toEqual(new Set([field, path, `${path}[0]`]))
    expect(changedRecordFields(complex)).toEqual(new Set([field]))
  })
})

describe('paired record row alignment', () => {
  it('aligns following fields after an inserted child and text wrapping on opposite sides', () => {
    const result = planDiffRowAlignment([
      { path: 'items', top: 0, height: 30 },
      { path: 'items[0]', top: 30, height: 60 },
      { path: 'note', top: 90, height: 30 },
    ], [
      { path: 'items', top: 0, height: 30 },
      { path: 'items[0]', top: 30, height: 30 },
      { path: 'items[1]', top: 60, height: 30 },
      { path: 'note', top: 90, height: 60 },
    ])
    expect(result.left[result.left.length - 1]).toEqual({ index: 2, before: 30, height: 60 })
    expect(result.right[1]).toEqual({ index: 1, before: 0, height: 60 })
    expect(result.right[result.right.length - 1]).toEqual({ index: 3, before: 0, height: 60 })
  })
})
