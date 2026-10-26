import { motionRecordKey } from '../../record-editing/hooks/useMutationFeedback'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, memo } from 'react'
import { Handle, Position, useReactFlow, useViewport, useInternalNode, type NodeProps, type Node } from '@xyflow/react'

import { useReferenceShortName } from '../../record-editing/components/ShortNameContext'

import type { CollectionEdit } from '../../../bindings/CollectionEdit'

import { type FieldPathSegment, type FieldValue, type GraphNodeView } from '../../../wire'

import { DataCardNode } from '../../record-editing/components/DataCard'
import { DiagBadge } from '../../diagnostics/components/DiagBadge'
import { typeColor } from '../../../shared/lib/typeColor'

import { GraphRowGeometry } from '../layout/rowGeometry'
import { GraphRowWindowContext } from '../layout/rowWindow'
import { type RelationProjection } from './GraphView.relations'

// ─── Node data ───────────────────────────────────────────────────────────────

// 节点渲染及几何测量独立于图谱布局调度。
export interface NodeData extends Record<string, unknown> {
  graphNode: GraphNodeView
  expanded: boolean
  /** Distinct edge field_paths whose source is this node (e.g. ["unlockGeneList[0]", "unlockGeneList[1]"]) */
  outgoingPaths: string[]
  relation: RelationProjection
  geometry: GraphRowGeometry
  compact: boolean
  /** Stable signature of the per-row expanded set, so CfdNode can re-measure
   *  handle Y positions only when something that affects row geometry changes. */
  rowExpandKey: string
  expandedPaths: ReadonlySet<string>
  onToggleExpand: () => void
  onRowToggle: (path: string, exp: boolean) => void
  onInteraction: (active: boolean) => void
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  /** Ctrl+click on a node body opens that record in the record view. */
  onCtrlClick?: () => void
  /** Visually mark this node as the current inspector selection. */
  selected?: boolean
  /** Record-level severity, drives the corner badge on the graph node. */
  diagSeverity?: 'error' | 'warning' | null
  onDiagBadgeClick?: () => void
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────
// 关系端口按真实行位置测量，集合展开与异步内容变化后仍准确落在对应元素上。

/** 缩略标签的基准字号与下限：从较大字号开始按可用空间自适应缩小。 */
const COMPACT_MAX_FONT = 52
const COMPACT_MIN_FONT = 14

// 缩略模式下完整的 Key/缩略名尽量填满节点；长名称逐级缩小避免被裁掉。
function CompactNodeLabel({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState(COMPACT_MAX_FONT)
  useLayoutEffect(() => {
    const label = ref.current
    if (!label) return
    // 文字实际占位由 Range 测量，不受 flex 居中导致的 scroll 尺寸失真影响。
    const textBounds = () => {
      const range = document.createRange()
      range.selectNodeContents(label)
      return range.getBoundingClientRect()
    }
    const fit = () => {
      const box = label.getBoundingClientRect()
      let low = COMPACT_MIN_FONT
      let high = COMPACT_MAX_FONT
      let best = COMPACT_MIN_FONT
      for (let step = 0; step < 8; step++) {
        const mid = (low + high) / 2
        label.style.fontSize = `${mid}px`
        const bounds = textBounds()
        if (bounds.width <= box.width + 1 && bounds.height <= box.height + 1) {
          best = mid
          low = mid
        } else {
          high = mid
        }
      }
      label.style.fontSize = `${best}px`
      setSize(best)
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(label)
    return () => observer.disconnect()
  }, [text])
  return <div ref={ref} className="gn-compact-key" style={{ fontSize: size }}>{text}</div>
}

// 图卡片优先展示业务名称，将技术标识独立放在次级行，避免挤占标题。
function GraphNodeHeader({
  recordId,
  shortName,
  actualType,
  diagSeverity,
  onDiagBadgeClick,
}: {
  recordId: string
  shortName?: string
  actualType: string
  diagSeverity?: 'error' | 'warning' | null
  onDiagBadgeClick?: () => void
}) {
  return (
    <div
      className="gn-header gn-graph-header"
      style={{ '--node-color': typeColor(actualType) } as React.CSSProperties}
    >
      <div className="gn-color-bar" />
      <div className="gn-graph-heading">
        <span className="gn-key" title={`${shortName || recordId} · ${actualType}`}>{shortName || recordId}</span>
        {shortName && shortName !== recordId && <span className="gn-graph-id" title={recordId}>{recordId}</span>}
      </div>
      {(diagSeverity === 'error' || diagSeverity === 'warning') && (
        <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
      )}
    </div>
  )
}

function CfdNode(props: NodeProps) {
  const { graphNode } = props.data as NodeData
  const shortName = useReferenceShortName(graphNode.actual_type, graphNode.record_id)
  return <CfdNodeContent id={props.id} data={props.data} shortName={shortName} />
}

// 查询 generation 变化只重新解析名称；名称未变时不重新执行卡片字段树。
const CfdNodeContent = memo(function CfdNodeContent({ id, data, shortName }: Pick<NodeProps, 'id' | 'data'> & { shortName?: string }) {
  const { graphNode: gn, expanded, outgoingPaths, relation, geometry, compact, rowExpandKey, expandedPaths, onToggleExpand, onRowToggle, onInteraction, onEdit, onCollectionEdit, onCtrlClick, selected, diagSeverity, onDiagBadgeClick } = data as NodeData
  const rootRef = useRef<HTMLDivElement>(null)
  const headerRef = useRef<HTMLDivElement>(null)
  const flow = useReactFlow()
  const viewport = useViewport()
  const internal = useInternalNode(id)
  const geometryVersion = useSyncExternalStore(geometry.subscribe, geometry.snapshot)
  const [canvasHeight, setCanvasHeight] = useState(900)
  const [pinnedKeys, setPinnedKeys] = useState<ReadonlySet<string>>(new Set())
  const graphRelations = useMemo(() => relation.ports.length > 0 ? {
    expandedPaths: relation.expanded, appendPaths: relation.appendPaths,
  } : undefined, [relation])
  const outgoingSet = useMemo(() => new Set(outgoingPaths), [outgoingPaths])
  const handles = useMemo(() => geometryHandles(geometry, outgoingPaths), [geometry, geometryVersion, outgoingPaths])
  const publishGeometry = useCallback(() => {
    // React Flow 的尺寸观察会从 DOM 重新收集端口；随后用公开 API 恢复完整逻辑几何。
    flow.updateNode(id, { handles, width: 280, height: geometry.height, measured: { width: 280, height: geometry.height } })
  }, [flow, id, handles, geometry, geometryVersion])

  const localTop = -viewport.y / viewport.zoom - (internal?.internals.positionAbsolute.y ?? 0)
  const [handleFirst, handleEnd] = geometry.range(localTop - 180, localTop + canvasHeight / viewport.zoom + 180)
  const lastReset = useRef({ geometry, version: geometry.resetVersion })
  const newReset = lastReset.current.geometry !== geometry || lastReset.current.version !== geometry.resetVersion
  const pinned = useMemo(() => new Set([...pinnedKeys].flatMap(key => {
    if (newReset && geometry.resetsPath(key)) return []
    const index = geometry.byKey.get(key)
    return index === undefined ? [] : [index]
  })), [pinnedKeys, geometry, newReset, geometryVersion])
  useLayoutEffect(() => {
    if (!newReset) return
    lastReset.current = { geometry, version: geometry.resetVersion }
    setPinnedKeys(previous => {
      const next = new Set([...previous].filter(key => !geometry.resetsPath(key)))
      return next.size === previous.size ? previous : next
    })
    if (!rootRef.current?.contains(document.activeElement)) onInteraction(false)
  }, [geometry, geometryVersion, newReset, onInteraction])
  const rowWindow = useMemo(() => ({ geometry, first: compact ? 0 : handleFirst, end: compact ? 0 : handleEnd, pinned }),
    [geometry, geometryVersion, compact, handleFirst, handleEnd, pinned])
  const visibleHandles = useMemo(() => {
    const visible = new Set(geometry.rows.slice(handleFirst, handleEnd).map(row => row.path))
    for (const index of pinned) visible.add(geometry.rows[index].path)
    return [...visible].filter(path => outgoingSet.has(path))
  }, [geometry, handleFirst, handleEnd, pinned, outgoingSet])

  const zoomRef = useRef(viewport.zoom)
  zoomRef.current = viewport.zoom
  const publishRef = useRef(publishGeometry)
  publishRef.current = publishGeometry
  const publishFrame = useRef(0)
  const schedulePublish = useCallback(() => {
    cancelAnimationFrame(publishFrame.current)
    publishFrame.current = requestAnimationFrame(() => publishRef.current())
  }, [])
  const measureRows = useCallback(() => {
    const root = rootRef.current
    if (!root) return
    const rows = new Map<string, number>()
    const dynamic = new Set<string>()
    root.querySelectorAll<HTMLElement>('.dc-row[data-field-path-wire], [data-geometry-key]').forEach(row => {
      const key = row.dataset.geometryKey ?? row.dataset.fieldPathWire!
      rows.set(key, row.getBoundingClientRect().height / zoomRef.current)
      if (row.matches(':focus-within') || row.querySelector('.string-value-error, [aria-invalid="true"]')) dynamic.add(key)
    })
    const style = getComputedStyle(root)
    geometry.measure(rows, headerRef.current ? {
      height: headerRef.current.getBoundingClientRect().height / zoomRef.current,
      top: parseFloat(style.borderTopWidth), bottom: parseFloat(style.borderBottomWidth),
    } : undefined, dynamic)
  }, [geometry])
  const rowObserver = useRef<ResizeObserver | null>(null)
  const observedRows = useRef(new Set<Element>())
  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    const canvas = root.closest<HTMLElement>('.react-flow')
    let previousSize = ''
    const observer = new ResizeObserver(entries => {
      measureRows()
      // 只有节点外框真正变动才重新发布全量端口；窗口切换只测量新挂载的行。
      if (entries.some(entry => entry.target === root)) {
        const size = `${root.offsetWidth}:${root.offsetHeight}`
        if (size !== previousSize) { previousSize = size; schedulePublish() }
      }
      if (canvas) setCanvasHeight(canvas.clientHeight)
    })
    rowObserver.current = observer
    observer.observe(root)
    if (canvas) observer.observe(canvas)
    if (headerRef.current) observer.observe(headerRef.current)
    return () => {
      observer.disconnect()
      rowObserver.current = null
      observedRows.current.clear()
      cancelAnimationFrame(publishFrame.current)
    }
  }, [geometry, measureRows, schedulePublish])
  useLayoutEffect(() => {
    const next = new Set<Element>(rootRef.current?.querySelectorAll<HTMLElement>('.dc-row, [data-geometry-key]') ?? [])
    for (const row of observedRows.current) if (!next.has(row)) rowObserver.current?.unobserve(row)
    for (const row of next) if (!observedRows.current.has(row)) rowObserver.current?.observe(row)
    observedRows.current = next
    measureRows()
  }, [geometry, geometryVersion, compact, handleFirst, handleEnd, pinned, rowExpandKey, viewport.zoom, measureRows])
  useLayoutEffect(schedulePublish, [handles, geometryVersion, schedulePublish])

  const interactionFrame = useRef(0)
  const pointerCleanup = useRef<(() => void) | null>(null)
  useEffect(() => () => {
    cancelAnimationFrame(interactionFrame.current)
    pointerCleanup.current?.()
    onInteraction(false)
  }, [onInteraction])
  function releaseInteraction() {
    if (!rootRef.current?.contains(document.activeElement)) onInteraction(false)
  }
  function beginPointerInteraction(target: EventTarget | null) {
    if (!(target instanceof Element) || !target.closest('[data-field-path-wire], [data-add-path-wire], [data-handleid]')) return
    pinInteraction(target)
    onInteraction(true)
    pointerCleanup.current?.()
    const cleanup = () => {
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
      pointerCleanup.current = null
    }
    const end = () => { cleanup(); releaseInteraction() }
    pointerCleanup.current = cleanup
    window.addEventListener('pointerup', end)
    window.addEventListener('pointercancel', end)
  }

  function pinInteraction(target: EventTarget | null) {
    if (!(target instanceof Element)) return
    const row = target.closest<HTMLElement>('[data-field-path-wire], [data-add-path-wire], [data-handleid]')
    const path = row?.dataset.fieldPathWire
      ?? (row?.dataset.addPathWire ? `append:${row.dataset.addPathWire}` : undefined)
      ?? (row?.dataset.handleid?.startsWith('path-') ? geometry.rows[geometry.byPath.get(row.dataset.handleid.slice(5)) ?? -1]?.key : undefined)
    if (path) {
      const active = document.activeElement?.closest<HTMLElement>('[data-field-path-wire]')?.dataset.fieldPathWire
      const next = new Set(active ? [path, active] : [path])
      setPinnedKeys(previous => previous.size === next.size && [...next].every(key => previous.has(key)) ? previous : next)
    }
  }

  return (
    <div
      ref={rootRef}
      className={`graph-node${compact ? ' compact' : ''}${gn.in_focus_file ? ' focused' : ' dim'}${selected ? ' selected' : ''}`}
      data-nodeid={gn.id}
      data-motion-record={motionRecordKey(gn.file_path, gn.coordinate)}
      data-geometry-pending={geometry.pendingCount}
      style={{'--node-color': typeColor(gn.actual_type), height: geometry.height} as React.CSSProperties}
      onPointerDownCapture={event => beginPointerInteraction(event.target)}
      onFocusCapture={event => { cancelAnimationFrame(interactionFrame.current); pinInteraction(event.target); onInteraction(true) }}
      onBlurCapture={() => { cancelAnimationFrame(interactionFrame.current); interactionFrame.current = requestAnimationFrame(releaseInteraction) }}
      onDragStartCapture={() => onInteraction(true)}
      onDragEndCapture={releaseInteraction}
      onClick={e => {
        // Ctrl+click (or Cmd+click on macOS) opens the record. Plain click
        // is left for React Flow's selection/drag handling.
        if ((e.ctrlKey || e.metaKey) && onCtrlClick) {
          e.preventDefault()
          e.stopPropagation()
          onCtrlClick()
        }
      }}
      title={onCtrlClick ? `${gn.record_id} — Ctrl+点击打开记录` : gn.record_id}
    >
      <Handle type="target" position={Position.Left} id="__in" isConnectableStart={false} style={{ top: geometry.headerHeight / 2 }} />
      {/* 屏外端口保留在 Node.handles 中；仅可见和活动端口挂载交互 DOM。 */}
      {visibleHandles.map(path => (
        <Handle
          key={`src-${path}`}
          type="source"
          position={Position.Right}
          id={`path-${path}`}
          isConnectable={!!onEdit && !!relation.byId.get(path) && !relation.byId.get(path)!.readOnly && (!relation.byId.get(path)!.append || !!onCollectionEdit)}
          title={relation.byId.get(path)?.append ? '追加引用' : path}
          style={{ top: geometry.center(path) - geometry.borderTop, bottom: 'auto' }}
        />
      ))}
      {compact && (
        <div className="gn-compact-body">
          {/* 缩略模式只显示缩略名或 ID，不显示“缩略名(ID)”。 */}
          <CompactNodeLabel text={shortName ?? gn.record_id} />
          {(diagSeverity === 'error' || diagSeverity === 'warning') && (
            <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
          )}
        </div>
      )}
      {/* 缩略卸载字段明细，完整高度与锚点由几何模型保留。 */}
      <div className="gn-detail" style={compact ? { visibility: 'hidden', pointerEvents: 'none' } : undefined} inert={compact}>
          <div ref={headerRef}>
            <GraphNodeHeader
              recordId={gn.record_id}
              shortName={shortName}
              actualType={gn.actual_type}
              diagSeverity={diagSeverity}
              onDiagBadgeClick={onDiagBadgeClick}
            />
          </div>
          {gn.is_collapsed ? (
            <div className="gn-collapsed" data-geometry-key="collapsed">折叠（超出深度）</div>
          ) : (
            <GraphRowWindowContext.Provider value={rowWindow}><DataCardNode
              fields={gn.fields}
              actualType={gn.actual_type}
              showAll={expanded}
              onToggle={onToggleExpand}
              onRowToggle={onRowToggle}
              expandedPaths={expandedPaths}
              onEdit={onEdit}
              onCollectionEdit={onCollectionEdit}
              graphRelations={graphRelations}
            /></GraphRowWindowContext.Provider>
          )}
      </div>
    </div>
  )
})

export function geometryHandles(geometry: GraphRowGeometry, outgoingPaths: string[]) {
  return [
    // 与 CSS 的 16px Handle 及 1px 左右边框一致，保持公开几何与实际命中框相同。
    { id: '__in', type: 'target' as const, position: Position.Left, x: -7, y: geometry.borderTop + geometry.headerHeight / 2 - 8, width: 16, height: 16 },
    ...outgoingPaths.map(path => ({ id: `path-${path}`, type: 'source' as const, position: Position.Right,
      x: 271, y: geometry.center(path) - 8, width: 16, height: 16 })),
  ]
}

/** 屏外节点也按帧完成未知行高，每帧最多取 12 个样本，不同步挂载整棵字段树。 */
export function GraphGeometryProbeHost({ nodes }: { nodes: Node[] }) {
  const [batch, setBatch] = useState(0)
  const node = nodes.find(node => (node.data as NodeData).geometry.pendingCount > 0)
  const advance = useCallback(() => setBatch(value => value + 1), [])
  return node ? <GraphGeometryProbe key={`${node.id}:${batch}`} id={node.id} data={node.data as NodeData} onMeasured={advance} /> : null
}

function GraphGeometryProbe({ id, data, onMeasured }: { id: string; data: NodeData; onMeasured: () => void }) {
  const { graphNode: node, geometry, relation } = data
  const root = useRef<HTMLDivElement>(null)
  const flow = useReactFlow()
  const shortName = useReferenceShortName(node.actual_type, node.record_id)
  const [pinned] = useState(() => new Set(geometry.probes(12)))
  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    const rows = new Map<string, number>()
    element.querySelectorAll<HTMLElement>('.dc-row[data-field-path-wire], [data-geometry-key]').forEach(row => {
      rows.set(row.dataset.geometryKey ?? row.dataset.fieldPathWire!, row.getBoundingClientRect().height)
    })
    const header = element.querySelector<HTMLElement>('.gn-header')!
    const style = getComputedStyle(element)
    geometry.measure(rows, { height: header.getBoundingClientRect().height, top: parseFloat(style.borderTopWidth), bottom: parseFloat(style.borderBottomWidth) })
    const frame = requestAnimationFrame(() => {
      flow.updateNode(id, { handles: geometryHandles(geometry, data.outgoingPaths), width: 280, height: geometry.height, measured: { width: 280, height: geometry.height } })
      onMeasured()
    })
    return () => cancelAnimationFrame(frame)
  }, [geometry, pinned, flow, id, data, onMeasured])
  return <div ref={root} className="graph-node graph-geometry-probe" aria-hidden inert
    style={{ position: 'fixed', left: -10000, top: 0, width: 280, visibility: 'hidden', pointerEvents: 'none' }}>
    <GraphNodeHeader recordId={node.record_id} shortName={shortName} actualType={node.actual_type} />
    {node.is_collapsed ? <div className="gn-collapsed" data-geometry-key="collapsed">折叠（超出深度）</div>
      : <GraphRowWindowContext.Provider value={{ geometry, first: 0, end: 0, pinned }}>
        <DataCardNode fields={node.fields} actualType={node.actual_type} showAll={data.expanded} onToggle={data.onToggleExpand}
          expandedPaths={data.expandedPaths} onRowToggle={data.onRowToggle} onEdit={data.onEdit} onCollectionEdit={data.onCollectionEdit}
          graphRelations={relation.ports.length ? { expandedPaths: relation.expanded, appendPaths: relation.appendPaths } : undefined} />
      </GraphRowWindowContext.Provider>}
  </div>
}

// 卡片不读取坐标；移动节点只更新外层变换，不重新执行字段树和端口测量。
const CfdNodeMemo = memo(CfdNode, (previous, next) => previous.id === next.id && previous.data === next.data)
export const nodeTypes = { cfd: CfdNodeMemo }
