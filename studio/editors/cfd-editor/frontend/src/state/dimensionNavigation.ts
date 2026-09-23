import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { FileTreeNode } from '../bindings/FileTreeNode'
import type { FileTypeOption } from '../bindings/FileTypeOption'
import type { DimensionFileRecords } from '../bindings/DimensionFileRecords'

export interface DimensionTarget {
  dimension: string
  ownerFile: string
  typeName: string
  field: string | null
  singleton: boolean
}

export interface DimensionTreeNode {
  id: string
  label: string
  kind: 'directory' | 'file' | 'type' | 'field' | 'singleton'
  target?: DimensionTarget
  children: DimensionTreeNode[]
}

export function dimensionTargetId(target: DimensionTarget): string {
  return JSON.stringify([target.dimension, target.ownerFile, target.typeName, target.field ?? ''])
}

/** 维度入口沿用业务文件的目录与文件名；只有确实含有记录的类型参与导航。 */
export function buildDimensionTree(
  nodes: FileTreeNode[],
  dimension: DimensionInfo,
  fileTypes: Record<string, FileTypeOption[] | undefined>,
): DimensionTreeNode[] {
  const walk = (node: FileTreeNode): DimensionTreeNode | null => {
    if (node.is_dir) {
      const children = node.children.flatMap(child => {
        const result = walk(child)
        return result ? [result] : []
      })
      return children.length ? { id: `directory:${dimension.name}:${node.path}`, label: node.name, kind: 'directory', children } : null
    }
    if (!node.in_data || !node.name.endsWith('.cfd')) return null
    const present = (fileTypes[node.path] ?? []).filter(type => type.record_count > 0)
    const relevant = present.filter(type => (type.dimension_fields[dimension.name] ?? []).length > 0)
    if (!relevant.length) return null
    const multiType = present.length > 1
    const children = relevant.flatMap(type => {
      const fields = type.dimension_fields[dimension.name] ?? []
      const singleton = type.is_singleton
      const target: DimensionTarget = {
        dimension: dimension.name, ownerFile: node.path, typeName: type.name,
        field: null, singleton,
      }
      const fieldNodes: DimensionTreeNode[] = singleton ? [] : fields.map(field => {
        const fieldTarget = { ...target, field }
        return { id: dimensionTargetId(fieldTarget), label: field, kind: 'field', target: fieldTarget, children: [] }
      })
      if (!multiType) return fieldNodes
      return [{
        id: singleton ? dimensionTargetId(target) : `type:${dimension.name}:${node.path}:${type.name}`,
        label: type.display_name, kind: singleton ? 'singleton' as const : 'type' as const,
        target: singleton ? target : undefined, children: fieldNodes,
      }]
    })
    const single = relevant.length === 1 && relevant[0].is_singleton && !multiType
    const type = relevant[0]
    const target = single && type ? {
      dimension: dimension.name, ownerFile: node.path, typeName: type.name, field: null, singleton: true,
    } : undefined
    return {
      id: target ? dimensionTargetId(target) : `file:${dimension.name}:${node.path}`,
      label: node.name, kind: target ? 'singleton' : 'file', target,
      children: single ? [] : children,
    }
  }
  return nodes.flatMap(node => {
    const result = walk(node)
    return result ? [result] : []
  })
}

export function selectDimensionRows(data: DimensionFileRecords, target: DimensionTarget): DimensionFileRecords {
  return {
    ...data,
    rows: data.rows.filter(row => row.owner_file_path === target.ownerFile
      && row.coordinate.actual_type === target.typeName
      && (target.singleton || row.field === target.field)),
  }
}
