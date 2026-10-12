import { describe, expect, it } from 'vitest'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { FileTreeNode } from '../bindings/FileTreeNode'
import { buildFileTreeGroups } from './FileTree'

function node(name: string, path: string, children: FileTreeNode[] = []): FileTreeNode {
  const root = path.replace(/\\/g, '/').split('/')[0]
  return {
    name,
    path,
    is_dir: children.length > 0,
    in_sources: true,
    in_schema: root === 'schema',
    in_data: root === 'data',
    first_source_descendant: null,
    children,
  }
}

function dimension(name: string, displayName: string): DimensionInfo {
  return { name, display_name: displayName, variants: [], fields: [] }
}

describe('buildFileTreeGroups', () => {
  it('keeps schema and data files while adding virtual dimension views', () => {
    const schema = node('schema', 'schema', [node('items.cft', 'schema/items.cft')])
    const data = node('data', 'data', [node('items.cfd', 'data/items.cfd')])
    const groups = buildFileTreeGroups(
      [schema, data],
      [
        dimension('platform', '平台'),
        dimension('language', '本地化'),
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
    expect(groups[2].dimensionNodes).toEqual([])
    expect(groups[3].dimensionNodes).toEqual([])
  })

  it('keeps data nodes when dimensions are present', () => {
    const data = node('data', 'data')

    const groups = buildFileTreeGroups([data], [dimension('language', '本地化')])
    expect(groups[1].nodes).toEqual([data])
    expect(groups[2].dimensionNodes).toEqual([])
  })

  it('keeps empty directories in their configured group', () => {
    const emptySchema = node('nested', 'schema/nested')
    emptySchema.is_dir = true
    const emptyData = node('empty', 'data/empty')
    emptyData.is_dir = true

    const groups = buildFileTreeGroups([
      node('schema', 'schema', [emptySchema]),
      node('data', 'data', [emptyData]),
    ], [])

    expect(groups[0].nodes[0]?.children).toEqual([emptySchema])
    expect(groups[1].nodes[0]?.children).toEqual([emptyData])
  })
})


describe('维度文件树', () => {
  const field = (source_type: string, source_field: string, is_singleton = false) => ({ source_type, source_field, is_singleton })
  const fileTypes = {
    'data/items.cfd': [{ name: 'Item', display_name: '物品', record_count: 2, is_singleton: false,
      dimension_fields: { language: ['name', 'description'], platform: ['icon'] } }],
    'data/mixed.cfd': [
      { name: 'Item', display_name: '物品', record_count: 1, is_singleton: false, dimension_fields: { language: ['name'] } },
      { name: 'Settings', display_name: '设置', record_count: 1, is_singleton: true, dimension_fields: { language: ['title', 'hint'] } },
    ],
    'data/settings.cfd': [{ name: 'Settings', display_name: '设置', record_count: 1, is_singleton: true,
      dimension_fields: { language: ['title', 'hint'] } }],
    'data/empty.cfd': [{ name: 'Item', display_name: '物品', record_count: 0, is_singleton: false, dimension_fields: {} }],
  }
  const tree = [node('data', 'data', Object.keys(fileTypes).map(path => node(path.split('/').pop()!, path)))]
  const language = { ...dimension('language', '本地化'), fields: [field('Item', 'name'), field('Settings', 'title', true)] }

  it('单类型省略类型层；多类型保留类型名；单例仅为记录入口', () => {
    const group = buildFileTreeGroups(tree, [language], fileTypes)[2]
    const files = group.dimensionNodes![0]!.children
    expect(files.map(file => file.label)).toEqual(['items.cfd', 'mixed.cfd', 'settings.cfd'])
    expect(files[0]!.children.map(field => field.label)).toEqual(['name', 'description'])
    expect(files[1]!.children.map(type => type.label)).toEqual(['物品', '设置'])
    expect(files[1]!.children[1]).toMatchObject({ kind: 'singleton', children: [] })
    expect(files[2]).toMatchObject({ kind: 'singleton', children: [] })
  })

  it('其他维度沿用同一业务文件树，只列出所属字段', () => {
    const platform = { ...dimension('platform', '平台'), fields: [field('Item', 'icon')] }
    const group = buildFileTreeGroups(tree, [platform], fileTypes)[2]
    expect(group.dimensionNodes![0]!.children.map(file => file.label)).toEqual(['items.cfd'])
    expect(group.dimensionNodes![0]!.children[0]!.children.map(field => field.label)).toEqual(['icon'])
  })
})
