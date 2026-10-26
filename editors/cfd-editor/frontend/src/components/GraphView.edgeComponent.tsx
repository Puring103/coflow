import { BaseEdge, type EdgeProps } from '@xyflow/react'
import { createContext, useContext, useMemo, type ReactNode, type MouseEvent as ReactMouseEvent } from 'react'
import { shaderEdgePath, type EdgeClipRect } from './GraphView.edges'

const EdgeViewportContext = createContext<EdgeClipRect | undefined>(undefined)

/** 发光只计算视口附近的像素，避免屏外长线产生巨大的滤镜中间表面。 */
export function GraphEdgeViewport({ left, top, width, height, children }: {
  left: number; top: number; width: number; height: number; children: ReactNode
}) {
  const rect = useMemo(() => ({ left, top, right: left + width, bottom: top + height }), [left, top, width, height])
  return <EdgeViewportContext.Provider value={rect}>{children}</EdgeViewportContext.Provider>
}

// Shader Graph 风格：端口先水平伸出一小段，再斜直线连接到目标端口前的短段；
// 折点按角度自适应圆角，避免锐角尖点。
export function ShaderEdge({ id, sourceX, sourceY, targetX, targetY, style, markerEnd }: EdgeProps) {
  const clip = useContext(EdgeViewportContext)
  const path = useMemo(() => shaderEdgePath(
    { x: sourceX, y: sourceY },
    { x: targetX, y: targetY },
  ), [sourceX, sourceY, targetX, targetY])
  const drawingPath = clip ? shaderEdgePath({ x: sourceX, y: sourceY }, { x: targetX, y: targetY }, undefined, clip) : path
  // 线体拖动：找到离按下点较近的重连锚点，把原生事件转发给它，复用 React Flow 的重连流程。
  const beginBodyDrag = (event: ReactMouseEvent<SVGPathElement>) => {
    if (event.button !== 0) return
    const group = event.currentTarget.closest('.react-flow__edge')
    if (!group) return
    const anchors = Array.from(group.querySelectorAll<SVGCircleElement>('.react-flow__edgeupdater'))
    let nearest: SVGCircleElement | null = null
    let nearestDistance = Infinity
    for (const anchor of anchors) {
      const rect = anchor.getBoundingClientRect()
      const dx = rect.left + rect.width / 2 - event.clientX
      const dy = rect.top + rect.height / 2 - event.clientY
      const squared = dx * dx + dy * dy
      if (squared < nearestDistance) { nearestDistance = squared; nearest = anchor }
    }
    if (!nearest) return
    event.preventDefault()
    event.stopPropagation()
    nearest.dispatchEvent(new MouseEvent('mousedown', {
      bubbles: true, cancelable: true, view: window,
      clientX: event.clientX, clientY: event.clientY, button: 0,
    }))
  }
  return (
    <>
      <BaseEdge path={drawingPath} style={style} markerEnd={markerEnd} id={id} />
      <path
        d={path}
        className="graph-edge-drag"
        fill="none"
        stroke="transparent"
        strokeWidth={22}
        onMouseDown={beginBodyDrag}
      />
    </>
  )
}
