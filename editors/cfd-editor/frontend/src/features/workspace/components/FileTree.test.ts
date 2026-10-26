import { describe, expect, it } from 'vitest'
import type { DimensionInfo } from '../../../bindings/DimensionInfo'
import type { FileTreeNode } from '../../../bindings/FileTreeNode'
import { buildFileTreeGroups } from './FileTree'

function node(name: string, path: string, children: FileTreeNode[] = []): FileTreeNode {
  return { name, path, children, is_dir: children.length > 0, in_sources: true,
    in_schema: path.startsWith('schema'), in_data: !path.startsWith('schema'),
    dimension_only: false, first_source_descendant: null }
}
function dimension(name: string, display_name: string, files: string[] = []): DimensionInfo {
  return { name, display_name, variants: [], fields: [], dir: `dimensions/${name}`, files }
}

describe('文件分树', () => {
  it('类型、基础数据和各维度按实际文件组织', () => {
    const base = node('items.cfd', 'data/items.cfd')
    const overlay = { ...node('items.cfd', 'dimensions/language/data/items.cfd'), dimension_only: true }
    const mixed = node('mixed.cfd', 'data/mixed.cfd')
    const groups = buildFileTreeGroups([
      node('schema', 'schema', [node('items.cft', 'schema/items.cft')]),
      node('data', 'data', [base, mixed]),
      node('dimensions', 'dimensions', [node('language', 'dimensions/language', [
        node('data', 'dimensions/language/data', [overlay]),
      ])]),
    ], [dimension('region', '地区'), dimension('language', '本地化', [overlay.path, mixed.path])])
    expect(groups.map(group => group.label)).toEqual(['类型', '数据', '本地化', '地区'])
    expect(JSON.stringify(groups[1].nodes)).not.toContain(overlay.path)
    expect(JSON.stringify(groups[1].nodes)).toContain(base.path)
    const files = groups[2].dimensionNodes!.flatMap(function walk(item): import('../state/dimensionNavigation').DimensionTreeNode[] {
      return item.kind === 'file' ? [item] : item.children.flatMap(walk)
    })
    expect(files.map(file => file.target?.filePath)).toEqual([mixed.path, overlay.path])
    expect(files.every(file => file.children.length === 0)).toBe(true)
  })
  it('保留配置的空基础目录', () => {
    const empty = { ...node('empty', 'data/empty'), is_dir: true }
    expect(buildFileTreeGroups([empty], [dimension('language', '本地化')])[1].nodes).toEqual([empty])
  })
  it('源码比较保留纯覆盖的实际文件入口', () => {
    const overlay = { ...node('scattered.cfd', 'data/scattered.cfd'), dimension_only: true }
    const groups = buildFileTreeGroups([overlay], [], true)
    expect(groups[1].nodes).toEqual([])
    expect(groups[2].nodes).toEqual([overlay])
  })
})
