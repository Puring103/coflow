import { describe, expect, it } from 'vitest'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { FileTreeNode } from '../bindings/FileTreeNode'
import { buildFileTreeGroups } from './FileTree'

function node(name: string, path: string, children: FileTreeNode[] = []): FileTreeNode {
  return {
    name,
    path,
    is_dir: children.length > 0,
    in_sources: true,
    first_source_descendant: null,
    children,
  }
}

function dimension(name: string, displayName: string, outDir: string): DimensionInfo {
  return { name, display_name: displayName, out_dir: outDir, variants: [], fields: [] }
}

describe('buildFileTreeGroups', () => {
  it('keeps schema first, then data, localization, and other configured dimensions', () => {
    const schema = node('schema', 'schema', [node('items.cft', 'schema/items.cft')])
    const data = node('data', 'data', [node('items.cfd', 'data/items.cfd')])
    const languageFile = node('Item_name.cfd', 'generated/lang/Item_name.cfd')
    const platformFile = node('Item_icon.cfd', 'generated/platform/Item_icon.cfd')
    const groups = buildFileTreeGroups(
      [
        node('平台', 'generated/platform', [platformFile]),
        node('本地化', 'generated/lang', [languageFile]),
        schema,
        data,
      ],
      [
        dimension('platform', '平台', 'generated/platform'),
        dimension('language', '本地化', 'generated\\lang\\'),
      ],
    )

    expect(groups.map(group => [group.label, group.icon])).toEqual([
      ['类型', 'code'],
      ['数据', 'data'],
      ['本地化', 'localization'],
      ['平台', 'dimension'],
    ])
    expect(groups[0].nodes).toEqual([schema])
    expect(groups[1].nodes).toEqual([data])
    expect(groups[2].nodes).toEqual([languageFile])
    expect(groups[3].nodes).toEqual([platformFile])
  })

  it('leaves unmatched nodes under data for older snapshots', () => {
    const data = node('data', 'data')

    expect(buildFileTreeGroups([data], [dimension('language', '本地化', 'missing')]))
      .toEqual([
        { key: '__schema__', label: '类型', icon: 'code', nodes: [] },
        { key: '__data__', label: '数据', icon: 'data', nodes: [data] },
      ])
  })
})
