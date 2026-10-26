import { useLayoutEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ReactFlow, Handle, Position, useReactFlow, type Node, type Edge, type NodeProps, applyNodeChanges } from '@xyflow/react'
import { ShaderEdge } from '../../src/features/graph/components/GraphView.edgeComponent'
import '@xyflow/react/dist/style.css'

// 原型只使用公开的 Node.handles、measured 和受控节点更新；不读写框架私有 store。
function GeometryNode({ id, data }: NodeProps) {
  const root = useRef<HTMLDivElement>(null)
  const flow = useReactFlow()
  useLayoutEffect(() => {
    let frame = 0
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => flow.updateNode(id, node => ({ handles: node.handles?.slice() })))
    })
    if (root.current) observer.observe(root.current)
    return () => { observer.disconnect(); cancelAnimationFrame(frame) }
  }, [flow, id])
  return <div ref={root} style={{ width: 280, height: Number(data.height), background: '#333', color: 'white' }}>
    {String(data.label)}
    {data.target && <Handle type="target" id="in" position={Position.Left} style={{ top: 21 }} />}
  </div>
}
const initial: Node[] = [
  { id: 'source', type: 'geometry', position: { x: 100, y: -1000 }, width: 280, height: 34000,
    measured: { width: 280, height: 34000 }, data: { label: 'No source Handle DOM', height: 34000 },
    handles: Array.from({ length: 1000 }, (_, i) => ({ id: `row-${i}`, type: 'source', position: Position.Right, x: 274, y: i * 34 + 15, width: 12, height: 12 })) },
  ...['target', 'other'].map((id, index): Node => ({ id, type: 'geometry', position: { x: 850, y: index * 250 + 150 },
    width: 280, height: 100, measured: { width: 280, height: 100 }, data: { label: id, height: 100, target: true },
    handles: [{ id: 'in', type: 'target', position: Position.Left, x: -6, y: 15, width: 12, height: 12 }] })),
]
const nodeTypes = { geometry: GeometryNode }, edgeTypes = { shader: ShaderEdge }
function Example() {
  const [nodes, setNodes] = useState(initial)
  const [edges, setEdges] = useState<Edge[]>([{ id: 'edge', source: 'source', sourceHandle: 'row-60', target: 'target', targetHandle: 'in', type: 'shader' }])
  const [events] = useState<unknown[]>([])
  Object.assign(window, { geometryHarness: { events, setNodes } })
  return <div style={{ width: '100vw', height: '100vh' }}><ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
    onInit={flow => Object.assign(window, { geometryFlow: flow })}
    onNodesChange={changes => setNodes(previous => applyNodeChanges(changes, previous))}
    onReconnect={(edge, connection) => { events.push(connection); setEdges(previous => previous.map(item => item.id === edge.id ? { ...item, ...connection } : item)) }}
    defaultViewport={{ x: 0, y: 0, zoom: 1 }} /></div>
}
createRoot(document.getElementById('root')!).render(<Example />)
