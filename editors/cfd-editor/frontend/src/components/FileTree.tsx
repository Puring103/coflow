import { useState } from "react";
import { api } from "../api";
import type { FileTreeNode } from "../bindings";

interface FileTreeProps {
  nodes: FileTreeNode[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onNewFile: () => void;
  sessionId: number;
}

interface TreeNodeProps {
  node: FileTreeNode;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  depth: number;
}

function TreeNode({ node, selectedPath, onSelect, depth }: TreeNodeProps) {
  const [open, setOpen] = useState(true);
  const indent = depth * 14;

  if (node.is_dir) {
    return (
      <div>
        <div
          onClick={() => setOpen(o => !o)}
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
            depth={depth + 1}
          />
        ))}
      </div>
    );
  }

  const isSelected = selectedPath === node.path;

  return (
    <div
      onClick={() => onSelect(node.path)}
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

export function FileTree({ nodes, selectedPath, onSelect, onNewFile, sessionId }: FileTreeProps) {
  void sessionId; // available for future use (e.g. drag-drop)
  void api; // imported for potential future direct use

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
            depth={0}
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
    </div>
  );
}
