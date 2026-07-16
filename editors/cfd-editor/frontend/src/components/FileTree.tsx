import { useRef, useState, useEffect } from 'react'
import type { FileTreeNode } from '../bindings/FileTreeNode'
import type { FileTypeOption } from '../bindings/FileTypeOption'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

interface Props {
  nodes: FileTreeNode[]
  fileTypes: Record<string, FileTypeOption[] | undefined>
  selectedFile: string | null
  selectedType: string
  onSelectFile: (path: string, typeName: string) => void
  onExitRight?: () => void
  onOpenSourceFile?: (path: string) => void
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
type FlatItem =
  | { kind: 'node'; node: FileTreeNode; depth: number }
  | { kind: 'type'; filePath: string; type: FileTypeOption; depth: number }

function visibleFlatItems(
  nodes: FileTreeNode[],
  collapsed: Set<string>,
  fileTypes: Record<string, FileTypeOption[] | undefined>,
  depth: number,
  out: FlatItem[] = [],
) {
  for (const n of nodes) {
    if (n.is_dir) {
      out.push({ kind: 'node', node: n, depth })
      if (!collapsed.has(n.path)) {
        visibleFlatItems(n.children, collapsed, fileTypes, depth + 1, out)
      }
    } else if (n.in_sources) {
      out.push({ kind: 'node', node: n, depth })
      const types = fileTypes[n.path] ?? []
      if (types.length > 1 && !collapsed.has(n.path)) {
        for (const type of types) {
          out.push({ kind: 'type', filePath: n.path, type, depth: depth + 1 })
        }
      }
    }
  }
  return out
}

export function FileTree({ nodes, fileTypes, selectedFile, selectedType, onSelectFile, onExitRight, onOpenSourceFile }: Props) {
  const rootRef = useRef<HTMLDivElement>(null)
  const [collapsed, setCollapsed] = useState<Set<string>>(() => loadCollapsed())
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null)
  const contextReturnPath = useRef<string | null>(null)

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
    const focused = document.activeElement as HTMLElement | null
    if ((e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && focused?.dataset.path) {
      const filePath = focused.dataset.filePath ?? focused.dataset.path
      const node = findNode(nodes, filePath)
      if (node && !node.is_dir && node.in_sources && onOpenSourceFile) {
        e.preventDefault()
        const rect = focused.getBoundingClientRect()
        contextReturnPath.current = node.path
        setContextMenu({ x: rect.left + 20, y: rect.top + rect.height, path: node.path })
      }
      return
    }
    if (
      e.key !== 'ArrowDown'
      && e.key !== 'ArrowUp'
      && e.key !== 'ArrowLeft'
      && e.key !== 'ArrowRight'
      && e.key !== 'Enter'
    ) return
    const flat = visibleFlatItems(nodes, collapsed, fileTypes, 0)
    if (flat.length === 0) return
    const cur = document.activeElement as HTMLElement | null
    const idx = flat.findIndex(item => flatItemPath(item) === cur?.dataset.path)
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      const next = flat[Math.min(idx + 1, flat.length - 1)]
      focusByPath(rootRef.current, flatItemPath(next))
      activateFlatItem(next, fileTypes, onSelectFile)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      const prev = flat[Math.max(idx - 1, 0)]
      focusByPath(rootRef.current, flatItemPath(prev))
      activateFlatItem(prev, fileTypes, onSelectFile)
    } else if (e.key === 'ArrowRight') {
      const item = flat[idx]
      if (!item) return
      e.preventDefault()
      if (item.kind === 'type') {
        onExitRight?.()
      } else {
        const expandable = item.node.is_dir || (fileTypes[item.node.path]?.length ?? 0) > 1
        if (!expandable) {
          onExitRight?.()
        } else if (collapsed.has(item.node.path)) {
          toggle(item.node.path)
        } else {
          const child = flat[idx + 1]
          if (child && child.depth > item.depth) {
            focusByPath(rootRef.current, flatItemPath(child))
            activateFlatItem(child, fileTypes, onSelectFile)
          } else {
            onExitRight?.()
          }
        }
      }
    } else if (e.key === 'ArrowLeft') {
      const item = flat[idx]
      if (!item) return
      e.preventDefault()
      if (item.kind === 'type') {
        focusByPath(rootRef.current, item.filePath)
      } else if (
        (item.node.is_dir || (fileTypes[item.node.path]?.length ?? 0) > 1)
        && !collapsed.has(item.node.path)
      ) {
        toggle(item.node.path)
      } else {
        const parent = findVisibleParent(flat, idx)
        if (parent) focusByPath(rootRef.current, flatItemPath(parent))
      }
    } else if (e.key === 'Enter') {
      const target = cur?.dataset.path
      const targetItem = flat.find(item => flatItemPath(item) === target)
      if (targetItem) {
        e.preventDefault()
        if (
          targetItem.kind === 'node'
          && (targetItem.node.is_dir || (fileTypes[targetItem.node.path]?.length ?? 0) > 1)
        ) {
          toggle(targetItem.node.path)
        } else {
          activateFlatItem(targetItem, fileTypes, onSelectFile)
        }
      }
    }
  }

  // Expand ancestors of the selected file when selection changes, so
  // navigation always reveals the target. We deliberately do NOT re-run this
  // on every `collapsed` change — otherwise a user collapsing an ancestor of
  // the currently-selected file would be immediately undone, which is the
  // bug that made top-level folders feel "uncollapsible".
  useEffect(() => {
    if (!selectedFile) return
    setCollapsed(prev => {
      let changed = false
      const next = new Set(prev)
      for (const n of nodes) {
        if (walkExpandIfParent(n, selectedFile, next)) changed = true
      }
      if ((fileTypes[selectedFile]?.length ?? 0) > 1 && next.delete(selectedFile)) changed = true
      if (!changed) return prev
      saveCollapsed(next)
      return next
    })
  }, [selectedFile, nodes, fileTypes])

  useEffect(() => {
    if (!contextMenu) return
    requestAnimationFrame(() => {
      rootRef.current?.querySelector<HTMLButtonElement>('.file-tree-context-menu .ctx-item')?.focus()
    })
    const close = (event: MouseEvent) => {
      if ((event.target as HTMLElement | null)?.closest('.file-tree-context-menu')) return
      setContextMenu(null)
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [contextMenu])

  return (
    <div className="file-tree" role="tree" aria-label="项目文件" onKeyDown={onKeyDown} ref={rootRef}>
      {nodes.map(n => (
        <TreeNode
          key={n.path}
          node={n}
          fileTypes={fileTypes}
          selectedFile={selectedFile}
          selectedType={selectedType}
          onSelectFile={onSelectFile}
          depth={0}
          collapsed={collapsed}
          onToggle={toggle}
          onContextMenu={(event, path) => {
            if (!onOpenSourceFile) return
            event.preventDefault()
            contextReturnPath.current = path
            setContextMenu({ x: event.clientX, y: event.clientY, path })
          }}
        />
      ))}
      {contextMenu && onOpenSourceFile && (
        <div
          className="context-menu file-tree-context-menu"
          role="menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onKeyDown={event => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            const path = contextReturnPath.current
            setContextMenu(null)
            if (path) requestAnimationFrame(() => focusByPath(rootRef.current, path))
          }}
        >
          <button
            type="button"
            className="ctx-item"
            role="menuitem"
            onClick={() => {
              const path = contextMenu.path
              setContextMenu(null)
              onOpenSourceFile(path)
            }}
          >
            <Icon name="open" size={13} aria-hidden />
            打开源文件
          </button>
        </div>
      )}
    </div>
  )
}

function findVisibleParent(
  items: ReturnType<typeof visibleFlatItems>,
  index: number,
) {
  const depth = items[index]?.depth ?? 0
  for (let i = index - 1; i >= 0; i -= 1) {
    if (items[i].depth < depth) return items[i]
  }
  return null
}

function flatItemPath(item: FlatItem): string {
  return item.kind === 'node' ? item.node.path : typeItemPath(item.filePath, item.type.name)
}

function typeItemPath(filePath: string, typeName: string): string {
  return `${filePath}\u001f${typeName}`
}

function activateFlatItem(
  item: FlatItem,
  fileTypes: Record<string, FileTypeOption[] | undefined>,
  onSelectFile: (path: string, typeName: string) => void,
) {
  if (item.kind === 'type') {
    onSelectFile(item.filePath, item.type.name)
    return
  }
  if (!item.node.is_dir && (fileTypes[item.node.path]?.length ?? 0) <= 1) {
    onSelectFile(item.node.path, fileTypes[item.node.path]?.[0]?.name ?? '')
  }
}

function findNode(nodes: FileTreeNode[], path: string): FileTreeNode | null {
  for (const node of nodes) {
    if (node.path === path) return node
    const child = findNode(node.children, path)
    if (child) return child
  }
  return null
}

/** If `node` is an ancestor directory of `targetFile`, remove it from
 *  `collapsed` (i.e. expand it) and recurse into children. Returns true when
 *  the set actually changed. */
function walkExpandIfParent(node: FileTreeNode, targetFile: string, collapsed: Set<string>): boolean {
  if (!node.is_dir) return false
  const prefix = node.path.endsWith('/') ? node.path : node.path + '/'
  if (!targetFile.startsWith(prefix)) return false
  let changed = false
  if (collapsed.delete(node.path)) changed = true
  for (const c of node.children) {
    if (walkExpandIfParent(c, targetFile, collapsed)) changed = true
  }
  return changed
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

function TreeNode({ node, fileTypes, selectedFile, selectedType, onSelectFile, depth, collapsed, onToggle, onContextMenu }: {
  node: FileTreeNode
  fileTypes: Record<string, FileTypeOption[] | undefined>
  selectedFile: string | null
  selectedType: string
  onSelectFile: (path: string, typeName: string) => void
  depth: number
  collapsed: Set<string>
  onToggle: (path: string) => void
  onContextMenu: (event: React.MouseEvent, path: string) => void
}) {
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
            fileTypes={fileTypes}
            selectedFile={selectedFile}
            selectedType={selectedType}
            onSelectFile={onSelectFile}
            depth={depth + 1}
            collapsed={collapsed}
            onToggle={onToggle}
            onContextMenu={onContextMenu}
          />
        ))}
      </div>
    )
  }

  const ghost = !node.in_sources
  const isCfd = node.name.endsWith('.cfd')
  const types = fileTypes[node.path] ?? []
  const hasTypeChildren = !ghost && types.length > 1
  const isCollapsed = collapsed.has(node.path)
  const selected = node.path === selectedFile && (types.length <= 1 || !selectedType)

  if (hasTypeChildren) {
    return (
      <div role="group">
        <div
          className={`tree-file tree-file-parent${node.path === selectedFile ? ' contains-selection' : ''}${isCfd ? ' is-cfd' : ''}`}
          style={{ paddingLeft: (depth + 1) * 12 + 8 }}
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={!isCollapsed}
          tabIndex={0}
          data-path={node.path}
          data-file-path={node.path}
          onClick={() => onToggle(node.path)}
          onContextMenu={event => onContextMenu(event, node.path)}
          title={node.path}
        >
          <Icon name={isCollapsed ? 'chevron-right' : 'chevron-down'} size={11} className="tree-file-chevron" aria-hidden />
          <Icon name={isCfd ? 'file-cfd' : 'file'} size={13} className="icon-file" aria-hidden />
          <span className="tree-item-label">{node.name}</span>
        </div>
        {!isCollapsed && types.map(type => (
          <div
            key={type.name}
            className={`tree-type${node.path === selectedFile && type.name === selectedType ? ' selected' : ''}`}
            style={{ paddingLeft: (depth + 2) * 12 + 20, '--type-color': typeColor(type.name) } as React.CSSProperties}
            role="treeitem"
            aria-level={depth + 2}
            aria-selected={node.path === selectedFile && type.name === selectedType}
            tabIndex={0}
            data-path={typeItemPath(node.path, type.name)}
            data-file-path={node.path}
            data-type-name={type.name}
            onClick={() => onSelectFile(node.path, type.name)}
            onContextMenu={event => onContextMenu(event, node.path)}
            title={type.display_name === type.name ? type.name : `${type.display_name} (${type.name})`}
          >
            <span className="tree-type-dot" aria-hidden />
            <span className="tree-item-label">{type.display_name}</span>
            <span className="tree-type-count">{type.record_count}</span>
          </div>
        ))}
      </div>
    )
  }

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
      data-file-path={node.path}
      data-type-name={types[0]?.name}
      onClick={() => !ghost && onSelectFile(node.path, types[0]?.name ?? '')}
      onContextMenu={event => { if (!ghost) onContextMenu(event, node.path) }}
      onKeyDown={e => {
        if (e.key === 'Enter' && !ghost) {
          e.preventDefault()
          e.stopPropagation()
          onSelectFile(node.path, types[0]?.name ?? '')
        }
      }}
      title={ghost ? '不在 sources 目录内（只读）' : node.path}
    >
      <Icon name={isCfd ? 'file-cfd' : 'file'} size={13} className="icon-file" aria-hidden />
      <span>{node.name}</span>
    </div>
  )
}
