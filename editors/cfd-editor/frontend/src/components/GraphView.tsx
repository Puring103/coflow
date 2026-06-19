import { useState, useCallback, useEffect, useRef } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
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
import { api } from "../api";

// ─── ELK layout ───────────────────────────────────────────────────────────────

const elk = new ELK();

const NODE_WIDTH = 200;
const NODE_HEIGHT_BASE = 80;
const NODE_HEIGHT_PER_FIELD = 18;
const NODE_HEIGHT_MAX = 260;

function nodeHeight(fieldCount: number): number {
  return Math.min(NODE_HEIGHT_BASE + fieldCount * NODE_HEIGHT_PER_FIELD, NODE_HEIGHT_MAX);
}

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
    children: gnodes.map(n => ({
      id: n.id,
      width: n.is_collapsed ? 44 : NODE_WIDTH,
      height: n.is_collapsed ? 44 : nodeHeight(n.fields.length),
    })),
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
  onExpand: (key: string) => void;
  onCollapse: (key: string) => void;
  onNavigate: (gnode: GraphNode) => void;
}

function CfdNode({ data }: NodeProps<Node<CfdNodeData>>) {
  const { gnode, color, isFocusFile, onContextMenu, onExpand, onCollapse, onNavigate } = data;

  if (gnode.is_collapsed) {
    return (
      <div
        onContextMenu={e => onContextMenu(e, gnode)}
        onClick={() => onExpand(gnode.key)}
        style={{
          width: 44,
          height: 44,
          borderRadius: "50%",
          background: "var(--bg3)",
          border: `2px solid ${color}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 12,
          color: "var(--text-muted)",
          cursor: "pointer",
          userSelect: "none",
        }}
        title={`${gnode.key} — click to expand`}
      >
        +
        <Handle type="target" position={Position.Left} style={{ opacity: 0 }} />
        <Handle type="source" position={Position.Right} style={{ opacity: 0 }} />
      </div>
    );
  }

  return (
    <div
      onContextMenu={e => onContextMenu(e, gnode)}
      onDoubleClick={() => onNavigate(gnode)}
      style={{
        width: NODE_WIDTH,
        minHeight: NODE_HEIGHT_BASE,
        background: "var(--bg2)",
        border: `2px solid ${isFocusFile ? color : color + "88"}`,
        borderRadius: 8,
        padding: 10,
        fontSize: 12,
        cursor: "pointer",
        boxShadow: isFocusFile ? `0 0 8px ${color}44` : "none",
      }}
      title="Double-click to open record"
    >
      <Handle type="target" position={Position.Left} style={{ background: color, opacity: 0.7 }} />
      <div style={{ marginBottom: 6, borderBottom: "1px solid var(--border)", paddingBottom: 4, display: "flex", alignItems: "flex-start", gap: 4 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div title={gnode.key} style={{ fontFamily: "monospace", fontWeight: 700, fontSize: 12, color: "var(--text)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {gnode.key}
          </div>
          <div style={{ color: color, fontSize: 10, marginTop: 2 }}>{gnode.actual_type}</div>
        </div>
        <button
          onClick={e => { e.stopPropagation(); onCollapse(gnode.key); }}
          title="Collapse node"
          style={{
            flexShrink: 0,
            width: 16,
            height: 16,
            padding: 0,
            background: "transparent",
            border: `1px solid ${color}66`,
            borderRadius: 3,
            color: "var(--text-muted)",
            fontSize: 10,
            lineHeight: "14px",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          −
        </button>
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

// ─── Layout cache (survives component unmount/remount) ────────────────────────

const layoutCache = new Map<string, Map<string, { x: number; y: number }>>();

/** Invalidate all cached layouts for a given (sessionId, filePath) pair. */
export function invalidateGraphCache(sessionId: number, filePath: string): void {
  const prefix = `${sessionId}:${filePath}:`;
  for (const key of Array.from(layoutCache.keys())) {
    if (key.startsWith(prefix)) layoutCache.delete(key);
  }
}

// ─── GraphView ────────────────────────────────────────────────────────────────

interface GraphViewProps {
  sessionId: number;
  filePath: string;
  onNavigate: (route: Route) => void;
  refreshKey?: number;
}

export function GraphView({ sessionId, filePath, onNavigate, refreshKey }: GraphViewProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<CfdNodeData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [search, setSearch] = useState("");
  const [layoutDone, setLayoutDone] = useState(false);
  const [graphData, setGraphData] = useState<GraphData | null>(null);
  const [expandedKeys, setExpandedKeys] = useState<string[]>([]);
  const [graphError, setGraphError] = useState<string | null>(null);
  const [layoutFallback, setLayoutFallback] = useState(false);
  const contextMenuRef = useRef<(e: React.MouseEvent, gnode: GraphNode) => void>(() => {});

  // Clear layout cache and reset state when session changes (new project loaded)
  useEffect(() => {
    for (const key of Array.from(layoutCache.keys())) {
      if (!key.startsWith(`${sessionId}:`)) layoutCache.delete(key);
    }
  }, [sessionId]);

  // Reset expanded keys, graph data, and search when file changes
  useEffect(() => {
    setExpandedKeys([]);
    setGraphData(null);
    setSearch("");
  }, [sessionId, filePath]);

  useEffect(() => {
    setGraphError(null);
    api.getGraph(sessionId, filePath, expandedKeys)
      .then(data => setGraphData(data))
      .catch(err => setGraphError(String(err)));
  // refreshKey is intentionally included — it lets the parent force a re-fetch after writes
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, filePath, expandedKeys, refreshKey]);

  const handleExpand = useCallback((key: string) => {
    setExpandedKeys(prev => prev.includes(key) ? prev : [...prev, key]);
  }, []);

  const handleCollapse = useCallback((key: string) => {
    setExpandedKeys(prev => prev.filter(k => k !== key));
  }, []);

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
        ...(!gnode.is_collapsed ? [{
          label: "折叠节点",
          onClick: () => handleCollapse(gnode.key),
        }] : [{
          label: "展开节点",
          onClick: () => handleExpand(gnode.key),
        }]),
      ],
    });
  }, [onNavigate, handleCollapse, handleExpand]);

  const expandRef = useRef<(key: string) => void>(() => {});
  const collapseRef = useRef<(key: string) => void>(() => {});
  const navigateRef = useRef<(gnode: GraphNode) => void>(() => {});
  // Keep refs up to date so node closures don't go stale
  contextMenuRef.current = handleNodeContextMenu;
  expandRef.current = handleExpand;
  collapseRef.current = handleCollapse;
  navigateRef.current = (gnode: GraphNode) =>
    onNavigate({ view: "record", file: gnode.file_path, recordKey: gnode.key });

  // Persist dragged positions back to cache on every node change
  const cacheKey = `${sessionId}:${filePath}:${[...expandedKeys].sort().join(",")}`;
  useEffect(() => {
    if (nodes.length === 0) return;
    const posMap = new Map<string, { x: number; y: number }>();
    for (const n of nodes) posMap.set(n.id, n.position);
    layoutCache.set(cacheKey, posMap);
    // Evict oldest entries if cache grows too large
    if (layoutCache.size > 50) {
      const firstKey = layoutCache.keys().next().value;
      if (firstKey !== undefined) layoutCache.delete(firstKey);
    }
  }, [nodes, cacheKey]);

  useEffect(() => {
    if (!graphData) return;
    setLayoutDone(false);
    setLayoutFallback(false);

    const stableHandler = (e: React.MouseEvent, gnode: GraphNode) =>
      contextMenuRef.current(e, gnode);
    const stableExpand = (key: string) => expandRef.current(key);
    const stableCollapse = (key: string) => collapseRef.current(key);
    const stableNavigate = (gnode: GraphNode) => navigateRef.current(gnode);

    const cached = layoutCache.get(cacheKey);
    const allCached = cached && graphData.nodes.every(n => cached.has(n.id));

    const applyPositions = (positions: Map<string, { x: number; y: number }>) => {
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
            onExpand: stableExpand,
            onCollapse: stableCollapse,
            onNavigate: stableNavigate,
          },
        };
      });
      return flowNodes;
    };

    const flowEdges: Edge[] = graphData.edges.map((e, i) => ({
      id: `e-${i}`,
      source: e.source,
      target: e.target,
      label: e.field_path,
      style: { stroke: "var(--text-muted)", strokeWidth: 1.5 },
      labelStyle: { fontSize: 10, fill: "var(--text-muted)" },
      labelBgStyle: { fill: "var(--bg2)" },
    }));

    if (allCached) {
      setNodes(applyPositions(cached!));
      setEdges(flowEdges);
      setLayoutDone(true);
      return;
    }

    layoutGraph(graphData.nodes, graphData.edges).then(positions => {
      setNodes(applyPositions(positions));
      setEdges(flowEdges);
      setLayoutDone(true);
    }).catch(_err => {
      setLayoutFallback(true);
      const fallbackPositions = new Map<string, { x: number; y: number }>();
      graphData.nodes.forEach((gnode, i) => {
        fallbackPositions.set(gnode.id, { x: (i % 4) * 260, y: Math.floor(i / 4) * 160 });
      });
      const flowNodes = applyPositions(fallbackPositions);
      const fallbackEdges: Edge[] = graphData.edges.map((e, i) => ({
        id: `e-${i}`,
        source: e.source,
        target: e.target,
        label: e.field_path,
        style: { stroke: "var(--text-muted)", strokeWidth: 1.5 },
      }));
      setNodes(flowNodes);
      setEdges(fallbackEdges);
      setLayoutDone(true);
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graphData, filePath, cacheKey]);

  // Apply search filter: highlight matching nodes, fade non-matching
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
        opacity: matches ? 1 : 0.15,
        filter: matches ? "drop-shadow(0 0 6px #4a9eff88)" : undefined,
      },
    };
  });

  const matchCount = search
    ? displayNodes.filter(n => ((n.style?.opacity as number | undefined) ?? 1) > 0.5).length
    : nodes.length;
  const noSearchMatches = search.length > 0 && matchCount === 0;

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
        <span style={{ color: "var(--text-muted)", fontSize: 12 }}>⌕</span>
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
        {graphData && expandedKeys.length > 0 && (
          <button
            onClick={() => setExpandedKeys([])}
            title="Collapse all expanded nodes"
            style={{ fontSize: 11, padding: "2px 8px", flexShrink: 0 }}
          >
            折叠全部
          </button>
        )}
        {graphData && (() => {
          const filePaths = Array.from(new Set(graphData.nodes.map(n => n.file_path))).filter(Boolean).sort();
          if (filePaths.length <= 1) return null;
          return (
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              {filePaths.map(fp => (
                <span key={fp} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 10, color: "var(--text-muted)" }}>
                  <span style={{ width: 8, height: 8, borderRadius: 2, background: fileColor(fp), flexShrink: 0 }} />
                  {fp.split("/").pop() ?? fp}
                </span>
              ))}
            </div>
          );
        })()}
        {graphData && (
          <span style={{ color: noSearchMatches ? "#ff5555" : "var(--text-muted)", fontSize: 12, marginLeft: "auto" }}>
            {search
              ? (noSearchMatches ? "No matches" : `${matchCount} / ${graphData.nodes.length} nodes`)
              : `${graphData.nodes.length} nodes · ${graphData.edges.length} edges`
            }
          </span>
        )}
      </div>

      {/* Flow canvas */}
      <div style={{ flex: 1, position: "relative" }}>
        {graphError && (
          <div style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "#ff5555",
            fontSize: 13,
            padding: 24,
            textAlign: "center",
            zIndex: 10,
          }}>
            Graph error: {graphError}
          </div>
        )}
        {!graphData && !graphError && (
          <div style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--text-muted)",
            fontSize: 14,
          }}>
            Loading…
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
        {layoutFallback && (
          <div style={{
            position: "absolute",
            top: 8,
            left: "50%",
            transform: "translateX(-50%)",
            background: "#ffb86c22",
            border: "1px solid #ffb86c88",
            color: "#ffb86c",
            borderRadius: 6,
            padding: "4px 12px",
            fontSize: 12,
            zIndex: 20,
            pointerEvents: "none",
          }}>
            ELK layout failed — using grid fallback
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
          <MiniMap
            nodeColor={n => {
              const data = n.data as CfdNodeData | undefined;
              return data?.color ?? "#888";
            }}
            style={{ background: "var(--bg2)", border: "1px solid var(--border)" }}
            maskColor="rgba(0,0,0,0.3)"
          />
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
