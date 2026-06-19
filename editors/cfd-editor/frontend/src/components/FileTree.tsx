import { useState, useRef, useCallback } from "react";
import type { FileTreeNode } from "../bindings";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";

interface FileTreeProps {
  nodes: FileTreeNode[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onNewFile: () => void;
  onDeleteFile?: (path: string) => void;
  sessionId?: number;
}

interface TreeNodeProps {
  node: FileTreeNode;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onContextMenu: (e: React.MouseEvent, node: FileTreeNode) => void;
  depth: number;
  expandedDirs: Set<string>;
  onToggleDir: (path: string) => void;
}

function TreeNode({ node, selectedPath, onSelect, onContextMenu, depth, expandedDirs, onToggleDir }: TreeNodeProps) {
  const indent = depth * 14;

  if (node.is_dir) {
    const open = expandedDirs.has(node.path);
    return (
      <div>
        <div
          onClick={() => onToggleDir(node.path)}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            padding: "2px 8px",
            paddingLeft: 8 + indent,
            cursor: "pointer",
            userSelect: "none",
            color: "var(--text)",
          }}
          onMouseEnter={e => (e.currentTarget.style.background = "var(--bg3)")}
          onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
        >
          <span style={{ fontSize: 10, color: "var(--text-muted)", width: 12, flexShrink: 0 }}>
            {open ? "▼" : "▶"}
          </span>
          <span style={{ fontSize: 12 }}>📁 {node.name}</span>
        </div>
        {open && node.children.map(child => (
          <TreeNode
            key={child.path}
            node={child}
            selectedPath={selectedPath}
            onSelect={onSelect}
            onContextMenu={onContextMenu}
            depth={depth + 1}
            expandedDirs={expandedDirs}
            onToggleDir={onToggleDir}
          />
        ))}
      </div>
    );
  }

  const isSelected = selectedPath === node.path;

  return (
    <div
      onClick={() => onSelect(node.path)}
      onContextMenu={e => { e.preventDefault(); onContextMenu(e, node); }}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "2px 8px",
        paddingLeft: 8 + indent + 16,
        cursor: "pointer",
        userSelect: "none",
        background: isSelected ? "var(--bg3)" : "transparent",
        color: node.in_sources ? "var(--text)" : "var(--text-muted)",
        borderLeft: isSelected ? "2px solid var(--accent)" : "2px solid transparent",
      }}
      onMouseEnter={e => {
        if (!isSelected) e.currentTarget.style.background = "var(--bg3)";
      }}
      onMouseLeave={e => {
        if (!isSelected) e.currentTarget.style.background = "transparent";
      }}
    >
      <span style={{ fontSize: 12, fontFamily: "monospace" }}>{node.name}</span>
      {!node.in_sources && (
        <span style={{ fontSize: 10, color: "var(--text-muted)", marginLeft: "auto" }}>
          (external)
        </span>
      )}
    </div>
  );
}

function collectDirPaths(nodes: FileTreeNode[]): string[] {
  const result: string[] = [];
  for (const n of nodes) {
    if (n.is_dir) {
      result.push(n.path);
      result.push(...collectDirPaths(n.children));
    }
  }
  return result;
}

export function FileTree({ nodes, selectedPath, onSelect, onNewFile, onDeleteFile }: FileTreeProps) {
  const expandedRef = useRef<Set<string> | null>(null);
  const knownDirsRef = useRef<Set<string>>(new Set());

  if (expandedRef.current === null) {
    const allDirs = collectDirPaths(nodes);
    expandedRef.current = new Set(allDirs);
    for (const d of allDirs) knownDirsRef.current.add(d);
  } else {
    // Auto-expand directories that are newly created (not seen before)
    for (const dirPath of collectDirPaths(nodes)) {
      if (!knownDirsRef.current.has(dirPath)) {
        expandedRef.current.add(dirPath);
        knownDirsRef.current.add(dirPath);
      }
    }
  }

  const [, forceRender] = useState(0);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  const handleToggleDir = useCallback((path: string) => {
    const s = expandedRef.current!;
    if (s.has(path)) s.delete(path); else s.add(path);
    forceRender(n => n + 1);
  }, []);

  const handleNodeContextMenu = useCallback((e: React.MouseEvent, node: FileTreeNode) => {
    const items = [];
    if (onDeleteFile) {
      items.push({
        label: "删除文件",
        danger: true,
        onClick: () => {
          if (window.confirm(`Delete file "${node.name}"? This cannot be undone.`)) {
            onDeleteFile(node.path);
          }
        },
      });
    }
    if (items.length === 0) return;
    setContextMenu({ x: e.clientX, y: e.clientY, items });
  }, [onDeleteFile]);

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      flex: 1,
      overflow: "hidden",
    }}>
      <div style={{
        padding: "6px 8px",
        fontSize: 11,
        fontWeight: 600,
        color: "var(--text-muted)",
        textTransform: "uppercase",
        letterSpacing: 1,
        borderBottom: "1px solid var(--border)",
      }}>
        Files
      </div>
      <div style={{ flex: 1, overflowY: "auto", paddingTop: 4 }}>
        {nodes.map(node => (
          <TreeNode
            key={node.path}
            node={node}
            selectedPath={selectedPath}
            onSelect={onSelect}
            onContextMenu={handleNodeContextMenu}
            depth={0}
            expandedDirs={expandedRef.current!}
            onToggleDir={handleToggleDir}
          />
        ))}
        {nodes.length === 0 && (
          <div style={{ padding: "8px 12px", color: "var(--text-muted)", fontSize: 12 }}>
            No files
          </div>
        )}
      </div>
      <div style={{ borderTop: "1px solid var(--border)", padding: 6 }}>
        <button
          onClick={onNewFile}
          style={{ width: "100%", fontSize: 12, justifyContent: "flex-start" }}
        >
          + New file
        </button>
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenu.items}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
