import { useRef, useState, useMemo } from 'react'
import type { FileTreeNode } from '../bindings/index'
import { Icon } from './Icon'

interface Props {
  nodes: FileTreeNode[]
  selectedFile: string | null
  onSelectFile: (path: string) => void
}

const COLLAPSE_KEY = 'cfd-editor-tree-collapsed'

/** Load the set of collapsed directory paths from localStorage. */
function loadCollapsed(): Set<string> {
  try {
    const raw = localStorage.getItem(COLLAPSE_KEY)
    if (!raw) return new Set()
    const arr = JSON.parse(raw) as string[]
    return new Set(arr)
  } catch {
    return new Set()
  }
}

function saveCollapsed(set: Set<string>) {
  try {
    localStorage.setItem(COLLAPSE_KEY, JSON.stringify(Array.from(set)))
  } catch {
    /* ignore quota / private mode */
  }
}

/** Walk only the nodes that are currently visible given the collapsed set. */
function visibleFlatItems(
  nodes: FileTreeNode[],
  collapsed: Set<string>,
  depth: number,
  out: { node: FileTreeNode; depth: number }[] = [],
) {
  for (const n of nodes) {
    if (n.is_dir) {
      out.push({ node: n, depth })
      if (!collapsed.has(n.path)) {
        visibleFlatItems(n.children, collapsed, depth + 1, out)
      }
    } else if (n.in_sources) {
      out.push({ node: n, depth })
    }
  }
  return out
}

export function FileTree({ nodes, selectedFile, onSelectFile }: Props) {
  const rootRef = useRef<HTMLDivElement>(null)
  const [collapsed, setCollapsed] = useState<Set<string>>(() => loadCollapsed())

  const toggle = (path: string) => {
    setCollapsed(prev => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      saveCollapsed(next)
      return next
    })
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Enter') return
    const flat = visibleFlatItems(nodes, collapsed, 0)
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
      const targetNode = flat.find(it => it.node.path === target)?.node
      if (targetNode) {
        e.preventDefault()
        if (targetNode.is_dir) {
          toggle(targetNode.path)
        } else if (targetNode.in_sources) {
          onSelectFile(targetNode.path)
        }
      }
    }
  }

  // Expand parents of the selected file so it stays visible after navigation.
  const expandedForSelection = useMemo(() => {
    if (!selectedFile) return collapsed
    const next = new Set(collapsed)
    for (const n of nodes) {
      walkExpandIfParent(n, selectedFile, next)
    }
    // Only persist if it actually changed to avoid thrashing localStorage.
    if (next.size !== collapsed.size) saveCollapsed(next)
    return next
  }, [collapsed, selectedFile, nodes])

  return (
    <div className="file-tree" role="tree" aria-label="项目文件" onKeyDown={onKeyDown} ref={rootRef}>
      {nodes.map(n => (
        <TreeNode
          key={n.path}
          node={n}
          selectedFile={selectedFile}
          onSelectFile={onSelectFile}
          depth={0}
          collapsed={expandedForSelection}
          onToggle={toggle}
        />
      ))}
    </div>
  )
}

/** If `node` is an ancestor directory of `targetFile`, remove it from
 *  `collapsed` (i.e. expand it) and recurse into children. */
function walkExpandIfParent(node: FileTreeNode, targetFile: string, collapsed: Set<string>) {
  if (!node.is_dir) return
  const prefix = node.path.endsWith('/') ? node.path : node.path + '/'
  if (!targetFile.startsWith(prefix)) return
  collapsed.delete(node.path)
  for (const c of node.children) walkExpandIfParent(c, targetFile, collapsed)
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

function TreeNode({ node, selectedFile, onSelectFile, depth, collapsed, onToggle }: {
  node: FileTreeNode
  selectedFile: string | null
  onSelectFile: (path: string) => void
  depth: number
  collapsed: Set<string>
  onToggle: (path: string) => void
}) {
  const selected = !node.is_dir && node.path === selectedFile

  if (node.is_dir) {
    const isCollapsed = collapsed.has(node.path)
    return (
      <div role="group">
        <div
          className="tree-dir-label"
          style={{ paddingLeft: depth * 12 + 8 }}
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={!isCollapsed}
          tabIndex={-1}
          data-path={node.path}
          onClick={() => onToggle(node.path)}
          onKeyDown={e => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              e.stopPropagation()
              onToggle(node.path)
            }
          }}
        >
          <Icon
            name={isCollapsed ? 'chevron-right' : 'chevron-down'}
            size={11}
            className="tree-dir-chevron"
            aria-hidden
          />
          <Icon name="folder" size={13} className="icon-folder" aria-hidden />
          <span>{node.name}</span>
        </div>
        {!isCollapsed && node.children.map(c => (
          <TreeNode
            key={c.path}
            node={c}
            selectedFile={selectedFile}
            onSelectFile={onSelectFile}
            depth={depth + 1}
            collapsed={collapsed}
            onToggle={onToggle}
          />
        ))}
      </div>
    )
  }

  const ghost = !node.in_sources
  const isCfd = node.name.endsWith('.cfd')
  return (
    <div
      className={`tree-file${selected ? ' selected' : ''}${ghost ? ' ghost' : ''}${isCfd ? ' is-cfd' : ''}`}
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
      <Icon name={isCfd ? 'file-cfd' : 'file'} size={13} className="icon-file" aria-hidden />
      <span>{node.name}</span>
    </div>
  )
}
