import type { FileTreeNode } from '../bindings/index'
import { Icon } from './Icon'

interface Props {
  nodes: FileTreeNode[]
  selectedFile: string | null
  onSelectFile: (path: string) => void
}

export function FileTree({ nodes, selectedFile, onSelectFile }: Props) {
  return (
    <div className="file-tree">
      {nodes.map(n => (
        <TreeNode
          key={n.path}
          node={n}
          selectedFile={selectedFile}
          onSelectFile={onSelectFile}
          depth={0}
        />
      ))}
    </div>
  )
}

function TreeNode({ node, selectedFile, onSelectFile, depth }: {
  node: FileTreeNode
  selectedFile: string | null
  onSelectFile: (path: string) => void
  depth: number
}) {
  const selected = !node.is_dir && node.path === selectedFile

  if (node.is_dir) {
    return (
      <div>
        <div className="tree-dir-label" style={{ paddingLeft: depth * 12 + 8 }}>
          <Icon name="folder" size={13} className="icon-folder" />
          <span>{node.name}</span>
        </div>
        {node.children.map(c => (
          <TreeNode key={c.path} node={c} selectedFile={selectedFile} onSelectFile={onSelectFile} depth={depth + 1} />
        ))}
      </div>
    )
  }

  return (
    <div
      className={`tree-file${selected ? ' selected' : ''}${!node.in_sources ? ' ghost' : ''}`}
      style={{ paddingLeft: (depth + 1) * 12 + 8 }}
      onClick={() => node.in_sources && onSelectFile(node.path)}
      title={!node.in_sources ? '不在 sources 目录内（只读）' : node.path}
    >
      <Icon name="file" size={13} className="icon-file" />
      <span>{node.name}</span>
    </div>
  )
}
