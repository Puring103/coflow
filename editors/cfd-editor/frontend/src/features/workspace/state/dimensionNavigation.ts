import type { DimensionInfo } from '../../../bindings/DimensionInfo'
import type { FileTreeNode } from '../../../bindings/FileTreeNode'

export interface DimensionTarget {
  dimension: string
  filePath: string
}

export interface DimensionTreeNode {
  id: string
  label: string
  kind: 'directory' | 'file'
  target?: DimensionTarget
  children: DimensionTreeNode[]
}

export function dimensionTargetId(target: DimensionTarget): string {
  return JSON.stringify([target.dimension, target.filePath])
}

export function dimensionViewPath(target: DimensionTarget): string {
  return `@dimension/${target.dimension}/${target.filePath}`
}

/** 按真实覆盖来源分树，混合文件和分散文件都使用实际文件路径。 */
export function buildDimensionTree(nodes: FileTreeNode[], dimension: DimensionInfo): DimensionTreeNode[] {
  const files = new Set(dimension.files)
  const walk = (node: FileTreeNode): DimensionTreeNode | null => {
    if (node.is_dir) {
      const children = node.children.flatMap(child => {
        const result = walk(child)
        return result ? [result] : []
      })
      return children.length ? {
        id: `directory:${dimension.name}:${node.path}`, label: node.name, kind: 'directory', children,
      } : null
    }
    if (!files.has(node.path)) return null
    const target = { dimension: dimension.name, filePath: node.path }
    return { id: dimensionTargetId(target), label: node.name, kind: 'file', target, children: [] }
  }
  return nodes.flatMap(node => {
    const result = walk(node)
    return result ? [result] : []
  })
}

/** 基础文件的创建入口只计算配置目录下的项目相对路径；实体文件在首次写入时创建。 */
export function suggestedDimensionTarget(dimension: DimensionInfo, ownerFile: string): DimensionTarget | null {
  if (!dimension.dir || ownerFile.startsWith('/') || /^[A-Za-z]:\//.test(ownerFile)
    || ownerFile.split('/').includes('..')) return null
  return { dimension: dimension.name, filePath: `${dimension.dir.replace(/\/$/, '')}/${ownerFile}` }
}
