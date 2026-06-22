import { useRef } from 'react'
import type { FileTreeNode } from '../bindings/index'
import { Icon } from './Icon'

interface Props {
  nodes: FileTreeNode[]
  selectedFile: string | null
  onSelectFile: (path: string) => void
}

/** Build a flat, depth-annotated list of *interactive* tree items (dirs +
 *  in-source files) in document order. Non-source files are skipped because
 *  they are not focusable. Used for arrow-key navigation. */
function flattenItems(nodes: FileTreeNode[], depth: number, out: { node: FileTreeNode; depth: number }[] = []) {
  for (const n of nodes) {
    if (n.is_dir) {
      out.push({ node: n, depth })
      flattenItems(n.children, depth + 1, out)
    } else if (n.in_sources) {
      out.push({ node: n, depth })
    }
  }
  return out
}

export function FileTree({ nodes, selectedFile, onSelectFile }: Props) {
  const rootRef = useRef<HTMLDivElement>(null)

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Enter') return
    const flat = flattenItems(nodes, 0)
    if (flat.length === 0) return
    const cur = document.activeElement as HTMLElement | null
    const idx = flat.findIndex(it => it.node.path === cur?.dataset.path)
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      const next = flat[Math.min(idx + 1, flat.length - 1)]
      focusByPath(rootRef.current, next.node.path)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      const prev = flat[Math.max(idx - 1, 0)]
      focusByPath(rootRef.current, prev.node.path)
    } else if (e.key === 'Enter') {
      const target = cur?.dataset.path
      if (target) {
        e.preventDefault()
        onSelectFile(target)
      }
    }
  }

  return (
    <div className="file-tree" role="tree" aria-label="项目文件" onKeyDown={onKeyDown} ref={rootRef}>
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

/** Find the treeitem element by data-path and focus it. */
function focusByPath(root: HTMLElement | null, path: string) {
  if (!root) return
  const el = root.querySelector<HTMLElement>(`[data-path="${cssEscape(path)}"]`)
  el?.focus()
}

function cssEscape(s: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s)
  return s.replace(/["\\]/g, '\\$&')
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
      <div role="group">
        <div
          className="tree-dir-label"
          style={{ paddingLeft: depth * 12 + 8 }}
          role="treeitem"
          aria-level={depth + 1}
          tabIndex={-1}
          data-path={node.path}
        >
          <Icon name="folder" size={13} className="icon-folder" aria-hidden />
          <span>{node.name}</span>
        </div>
        {node.children.map(c => (
          <TreeNode key={c.path} node={c} selectedFile={selectedFile} onSelectFile={onSelectFile} depth={depth + 1} />
        ))}
      </div>
    )
  }

  const ghost = !node.in_sources
  return (
    <div
      className={`tree-file${selected ? ' selected' : ''}${ghost ? ' ghost' : ''}`}
      style={{ paddingLeft: (depth + 1) * 12 + 8 }}
      role="treeitem"
      aria-level={depth + 1}
      aria-selected={selected}
      aria-disabled={ghost || undefined}
      tabIndex={ghost ? -1 : 0}
      data-path={node.path}
      onClick={() => !ghost && onSelectFile(node.path)}
      onKeyDown={e => {
        if (e.key === 'Enter' && !ghost) {
          e.preventDefault()
          e.stopPropagation()
          onSelectFile(node.path)
        }
      }}
      title={ghost ? '不在 sources 目录内（只读）' : node.path}
    >
      <Icon name="file" size={13} className="icon-file" aria-hidden />
      <span>{node.name}</span>
    </div>
  )
}
