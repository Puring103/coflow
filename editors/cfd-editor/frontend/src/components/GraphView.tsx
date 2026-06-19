import { useState, useCallback, useEffect, useRef } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  Handle,
  Position,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import ELK from "elkjs/lib/elk.bundled.js";
import type { GraphData, GraphNode, FieldCell } from "../bindings";
import type { Route } from "../router";
import { DataCard } from "./DataCard";
import { ContextMenu, type ContextMenuState } from "./ContextMenu";

// ─── ELK layout ───────────────────────────────────────────────────────────────

const elk = new ELK();

const NODE_WIDTH = 200;
const NODE_HEIGHT = 110;

async function layoutGraph(
  gnodes: GraphNode[],
  gedges: { source: string; target: string }[]
): Promise<Map<string, { x: number; y: number }>> {
  const graph = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.spacing.nodeNode": "40",
      "elk.layered.spacing.nodeNodeBetweenLayers": "60",
    },
    children: gnodes.map(n => ({ id: n.id, width: NODE_WIDTH, height: NODE_HEIGHT })),
    edges: gedges.map((e, i) => ({
      id: `e${i}`,
      sources: [e.source],
      targets: [e.target],
    })),
  };

  const layout = await elk.layout(graph);
  const positions = new Map<string, { x: number; y: number }>();
  for (const child of layout.children ?? []) {
    if (child.x !== undefined && child.y !== undefined) {
      positions.set(child.id, { x: child.x, y: child.y });
    }
  }
  return positions;
}

// ─── Color palette for files ──────────────────────────────────────────────────

const FILE_COLORS = [
  "#4a9eff",
  "#50fa7b",
  "#ffb86c",
  "#ff79c6",
  "#bd93f9",
  "#8be9fd",
  "#f1fa8c",
  "#ff5555",
];

function hashStr(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

function fileColor(filePath: string): string {
  return FILE_COLORS[hashStr(filePath) % FILE_COLORS.length];
}

// ─── Custom node ──────────────────────────────────────────────────────────────

interface CfdNodeData extends Record<string, unknown> {
  gnode: GraphNode;
  color: string;
  isFocusFile: boolean;
  onContextMenu: (e: React.MouseEvent, gnode: GraphNode) => void;
}

function CfdNode({ data }: NodeProps<Node<CfdNodeData>>) {
  const { gnode, color, isFocusFile, onContextMenu } = data;

  if (gnode.is_collapsed) {
    return (
      <div
        onContextMenu={e => onContextMenu(e, gnode)}
        style={{
          width: 40,
          height: 40,
          borderRadius: "50%",
          background: "var(--bg3)",
          border: `2px solid ${color}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 14,
          color: "var(--text-muted)",
          cursor: "context-menu",
        }}
        title={gnode.key}
      >
        …
        <Handle type="target" position={Position.Left} style={{ opacity: 0 }} />
        <Handle type="source" position={Position.Right} style={{ opacity: 0 }} />
      </div>
    );
  }

  return (
    <div
      onContextMenu={e => onContextMenu(e, gnode)}
      style={{
        width: NODE_WIDTH,
        minHeight: NODE_HEIGHT,
        background: "var(--bg2)",
        border: `2px solid ${isFocusFile ? color : color + "88"}`,
        borderRadius: 8,
        padding: 10,
        fontSize: 12,
        cursor: "context-menu",
        boxShadow: isFocusFile ? `0 0 8px ${color}44` : "none",
      }}
    >
      <Handle type="target" position={Position.Left} style={{ background: color, opacity: 0.7 }} />
      <div style={{ marginBottom: 6, borderBottom: "1px solid var(--border)", paddingBottom: 4 }}>
        <div style={{ fontFamily: "monospace", fontWeight: 700, fontSize: 12, color: "var(--text)" }}>
          {gnode.key}
        </div>
        <div style={{ color: color, fontSize: 10, marginTop: 2 }}>{gnode.actual_type}</div>
      </div>
      <div>
        <DataCard
          mode="node"
          value={{ kind: "Object", actual_type: gnode.actual_type, fields: gnode.fields as FieldCell[] }}
        />
      </div>
      <Handle type="source" position={Position.Right} style={{ background: color, opacity: 0.7 }} />
    </div>
  );
}

const nodeTypes = { cfd: CfdNode };

// ─── GraphView ────────────────────────────────────────────────────────────────

interface GraphViewProps {
  sessionId: number;
  filePath: string;
  graphData: GraphData | null;
  onNavigate: (route: Route) => void;
}

export function GraphView({ sessionId: _sessionId, filePath, graphData, onNavigate }: GraphViewProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<CfdNodeData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [search, setSearch] = useState("");
  const [layoutDone, setLayoutDone] = useState(false);
  const contextMenuRef = useRef<(e: React.MouseEvent, gnode: GraphNode) => void>(() => {});

  const handleNodeContextMenu = useCallback((e: React.MouseEvent, gnode: GraphNode) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: "跳转到记录视图",
          onClick: () => onNavigate({ view: "record", file: gnode.file_path, recordKey: gnode.key }),
        },
        {
          label: "在表中查看",
          onClick: () => onNavigate({ view: "table", file: gnode.file_path, typeFilter: gnode.actual_type }),
        },
      ],
    });
  }, [onNavigate]);

  // Keep ref up to date so the node closure doesn't go stale
  contextMenuRef.current = handleNodeContextMenu;

  useEffect(() => {
    if (!graphData) return;
    setLayoutDone(false);

    const stableHandler = (e: React.MouseEvent, gnode: GraphNode) =>
      contextMenuRef.current(e, gnode);

    layoutGraph(graphData.nodes, graphData.edges).then(positions => {
      const flowNodes: Node<CfdNodeData>[] = graphData.nodes.map(gnode => {
        const pos = positions.get(gnode.id) ?? { x: 0, y: 0 };
        return {
          id: gnode.id,
          type: "cfd",
          position: pos,
          data: {
            gnode,
            color: fileColor(gnode.file_path),
            isFocusFile: gnode.file_path === filePath || gnode.in_focus_file,
            onContextMenu: stableHandler,
          },
        };
      });

      const flowEdges: Edge[] = graphData.edges.map((e, i) => ({
        id: `e-${i}`,
        source: e.source,
        target: e.target,
        label: e.field_path,
        style: { stroke: "var(--text-muted)", strokeWidth: 1.5 },
        labelStyle: { fontSize: 10, fill: "var(--text-muted)" },
        labelBgStyle: { fill: "var(--bg2)" },
      }));

      setNodes(flowNodes);
      setEdges(flowEdges);
      setLayoutDone(true);
    }).catch(err => {
      console.error("ELK layout error:", err);
      // Fallback: simple grid layout
      const flowNodes: Node<CfdNodeData>[] = graphData.nodes.map((gnode, i) => ({
        id: gnode.id,
        type: "cfd",
        position: { x: (i % 4) * 260, y: Math.floor(i / 4) * 160 },
        data: {
          gnode,
          color: fileColor(gnode.file_path),
          isFocusFile: gnode.file_path === filePath || gnode.in_focus_file,
          onContextMenu: stableHandler,
        },
      }));
      const flowEdges: Edge[] = graphData.edges.map((e, i) => ({
        id: `e-${i}`,
        source: e.source,
        target: e.target,
        label: e.field_path,
        style: { stroke: "var(--text-muted)", strokeWidth: 1.5 },
      }));
      setNodes(flowNodes);
      setEdges(flowEdges);
      setLayoutDone(true);
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graphData, filePath]);

  // Apply search filter: fade non-matching nodes
  const displayNodes = nodes.map(node => {
    if (!search) return node;
    const g = node.data.gnode;
    const matches =
      g.key.toLowerCase().includes(search.toLowerCase()) ||
      g.actual_type.toLowerCase().includes(search.toLowerCase());
    return {
      ...node,
      style: {
        ...node.style,
        opacity: matches ? 1 : 0.2,
      },
    };
  });

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {/* Search bar */}
      <div style={{
        padding: "6px 12px",
        background: "var(--bg2)",
        borderBottom: "1px solid var(--border)",
        display: "flex",
        alignItems: "center",
        gap: 8,
        flexShrink: 0,
      }}>
        <span style={{ color: "var(--text-muted)", fontSize: 12 }}>🔍</span>
        <input
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder="Filter by key or type…"
          style={{
            background: "var(--bg3)",
            border: "1px solid var(--border)",
            borderRadius: 4,
            color: "var(--text)",
            padding: "3px 8px",
            fontSize: 12,
            outline: "none",
            width: 220,
          }}
        />
        {search && (
          <button onClick={() => setSearch("")} style={{ fontSize: 11, padding: "2px 6px" }}>✕</button>
        )}
        {graphData && (
          <span style={{ color: "var(--text-muted)", fontSize: 12, marginLeft: "auto" }}>
            {graphData.nodes.length} nodes · {graphData.edges.length} edges
          </span>
        )}
      </div>

      {/* Flow canvas */}
      <div style={{ flex: 1, position: "relative" }}>
        {!graphData && (
          <div style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--text-muted)",
            fontSize: 14,
          }}>
            No graph data available
          </div>
        )}
        {graphData && !layoutDone && (
          <div style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--text-muted)",
            fontSize: 14,
            zIndex: 10,
            background: "rgba(0,0,0,0.3)",
          }}>
            Computing layout…
          </div>
        )}
        <ReactFlow
          nodes={displayNodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.1 }}
          minZoom={0.1}
          maxZoom={2}
          style={{ background: "var(--bg)" }}
          onPaneClick={() => setContextMenu(null)}
        >
          <Background color="var(--bg3)" gap={20} />
          <Controls />
        </ReactFlow>
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
