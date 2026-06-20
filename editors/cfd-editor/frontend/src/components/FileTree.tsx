import { useState, useRef, useCallback } from "react";
import type { FileTreeNode } from "../bindings";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";
import { api } from "../api";

interface FileTreeProps {
  nodes: FileTreeNode[];
  selectedPath: string | null;
  sessionId?: number;
  onSelect: (path: string) => void;
  onNewFile: () => void;
  onDeleteFile?: (path: string) => void;
  onRenameFile?: (oldPath: string, newPath: string) => Promise<void>;
  onReloadFile?: (path: string) => void;
  onError?: (msg: string) => void;
}

interface RenameFileModal {
  node: FileTreeNode;
  draft: string;
  error: string | null;
}

interface DeleteFileModal {
  node: FileTreeNode;
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
          <span style={{ fontSize: 12 }}>▸ {node.name}</span>
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
      onClick={node.in_sources ? () => onSelect(node.path) : undefined}
      onContextMenu={e => { e.preventDefault(); onContextMenu(e, node); }}
      title={!node.in_sources ? `${node.path} (not in sources — read-only view not supported)` : node.path}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "2px 8px",
        paddingLeft: 8 + indent + 16,
        cursor: node.in_sources ? "pointer" : "default",
        userSelect: "none",
        background: isSelected ? "var(--bg3)" : "transparent",
        color: node.in_sources ? "var(--text)" : "var(--text-muted)",
        borderLeft: isSelected ? "2px solid var(--accent)" : "2px solid transparent",
        opacity: node.in_sources ? 1 : 0.5,
      }}
      onMouseEnter={e => {
        if (node.in_sources && !isSelected) e.currentTarget.style.background = "var(--bg3)";
      }}
      onMouseLeave={e => {
        if (!isSelected) e.currentTarget.style.background = "transparent";
      }}
    >
      <span style={{ fontSize: 12, fontFamily: "monospace" }}>{node.name}</span>
      {!node.in_sources ? (
        <span style={{ fontSize: 10, color: "var(--text-muted)", marginLeft: "auto" }}>
          (external)
        </span>
      ) : node.record_count > 0 ? (
        <span style={{ fontSize: 10, color: "var(--text-muted)", marginLeft: "auto", opacity: 0.7 }}>
          {node.record_count}
        </span>
      ) : null}
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

export function FileTree({ nodes, selectedPath, sessionId, onSelect, onNewFile, onDeleteFile, onRenameFile, onReloadFile, onError }: FileTreeProps) {
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
  const [renameModal, setRenameModal] = useState<RenameFileModal | null>(null);
  const [deleteFileModal, setDeleteFileModal] = useState<DeleteFileModal | null>(null);

  const handleToggleDir = useCallback((path: string) => {
    const s = expandedRef.current!;
    if (s.has(path)) s.delete(path); else s.add(path);
    forceRender(n => n + 1);
  }, []);

  const handleRenameCommit = useCallback(async () => {
    if (!renameModal || !onRenameFile) return;
    const newName = renameModal.draft.trim();
    if (!newName) { setRenameModal(m => m && ({ ...m, error: "Name cannot be empty" })); return; }
    if (!newName.endsWith(".cfd")) { setRenameModal(m => m && ({ ...m, error: "File name must end with .cfd" })); return; }
    if (newName === renameModal.node.name) { setRenameModal(null); return; }
    // Compute new rel path by replacing the last segment
    const oldPath = renameModal.node.path;
    const lastSlash = Math.max(oldPath.lastIndexOf("/"), oldPath.lastIndexOf("\\"));
    const newPath = lastSlash >= 0 ? oldPath.slice(0, lastSlash + 1) + newName : newName;
    try {
      await onRenameFile(oldPath, newPath);
      setRenameModal(null);
    } catch (e) {
      setRenameModal(m => m && ({ ...m, error: String(e) }));
    }
  }, [renameModal, onRenameFile]);

  const handleNodeContextMenu = useCallback((e: React.MouseEvent, node: FileTreeNode) => {
    const items: { label: string; danger?: boolean; onClick: () => void }[] = [];
    if (sessionId !== undefined) {
      items.push({
        label: "在资源管理器中显示",
        onClick: () => api.revealInExplorer(sessionId, node.path).catch(e => onError?.(`无法打开资源管理器: ${e}`)),
      });
    }
    if (onReloadFile && !node.is_dir) {
      items.push({
        label: "从磁盘重新加载",
        onClick: () => onReloadFile(node.path),
      });
    }
    if (onRenameFile && node.in_sources) {
      items.push({
        label: "重命名文件",
        onClick: () => setRenameModal({ node, draft: node.name, error: null }),
      });
    }
    if (onDeleteFile) {
      items.push({
        label: "删除文件",
        danger: true,
        onClick: () => setDeleteFileModal({ node }),
      });
    }
    if (items.length === 0) return;
    setContextMenu({ x: e.clientX, y: e.clientY, items });
  }, [onDeleteFile, onRenameFile, onReloadFile, sessionId]);

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

      {renameModal && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.6)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 2000,
          }}
          onClick={() => setRenameModal(null)}
        >
          <div
            style={{
              background: "var(--bg2)",
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: 24,
              width: 380,
              display: "flex",
              flexDirection: "column",
              gap: 12,
            }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>重命名文件</h3>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
              新文件名
              <input
                value={renameModal.draft}
                onChange={e => setRenameModal(m => m && ({ ...m, draft: e.target.value, error: null }))}
                onKeyDown={e => {
                  if (e.key === "Enter") { e.preventDefault(); handleRenameCommit(); }
                  if (e.key === "Escape") setRenameModal(null);
                  e.stopPropagation();
                }}
                style={{
                  background: "var(--bg3)",
                  border: renameModal.error ? "1px solid #ff5555" : "1px solid var(--border)",
                  borderRadius: 4,
                  color: "var(--text)",
                  padding: "4px 8px",
                  fontSize: 13,
                  fontFamily: "monospace",
                  outline: "none",
                }}
                autoFocus
              />
              {renameModal.error && (
                <span style={{ color: "#ff5555", fontSize: 11 }}>{renameModal.error}</span>
              )}
            </label>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setRenameModal(null)}>取消</button>
              <button
                className="primary"
                onClick={handleRenameCommit}
                disabled={!renameModal.draft.trim()}
              >
                重命名
              </button>
            </div>
          </div>
        </div>
      )}

      {deleteFileModal && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.6)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 2000,
          }}
          onClick={() => setDeleteFileModal(null)}
        >
          <div
            style={{
              background: "var(--bg2)",
              border: "1px solid var(--border)",
              borderRadius: 8,
              padding: 24,
              width: 380,
              display: "flex",
              flexDirection: "column",
              gap: 12,
            }}
            onClick={e => e.stopPropagation()}
          >
            <h3 style={{ margin: 0, fontSize: 15 }}>删除文件</h3>
            <p style={{ margin: 0, fontSize: 13, color: "var(--text-muted)" }}>
              Delete <strong style={{ color: "var(--text)", fontFamily: "monospace" }}>{deleteFileModal.node.name}</strong>? This cannot be undone.
            </p>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button onClick={() => setDeleteFileModal(null)}>取消</button>
              <button
                className="danger"
                onClick={() => {
                  onDeleteFile?.(deleteFileModal.node.path);
                  setDeleteFileModal(null);
                }}
              >
                删除
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
