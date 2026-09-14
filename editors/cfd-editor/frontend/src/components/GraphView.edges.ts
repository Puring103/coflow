/**
 * Shader Graph 风格连线几何。
 *
 * 路径由三段组成：从源端口水平伸出一小段 → 一条斜直线连接 → 水平进入目标端口。
 * 两个折点按内角大小做圆角：内角越接近直线越接近直角，越尖锐圆角越大，
 * 从而避免锐角直接显示为尖点。几何计算独立于 React Flow，便于单测。
 */

export interface EdgePoint {
  x: number
  y: number
}

/** 端口水平伸出的固定长度（flow 坐标）。 */
export const EDGE_STUB = 28
/** 起点内角取满圆角，连最接近直线的钝角也会得到轻微圆角（正直线时半径自然为 0）。 */
const CORNER_START = Math.PI
/** 内角小于该值后圆角达到最大（一半短边）；越尖锐的折角用越大的曲度。 */
const CORNER_MIN = Math.PI * 0.6
/** 短路段的圆角上限比例，避免两个折点的圆角相互覆盖。 */
const MAX_RADIUS_RATIO = 0.5

function subtract(a: EdgePoint, b: EdgePoint): EdgePoint {
  return { x: a.x - b.x, y: a.y - b.y }
}

function length(v: EdgePoint): number {
  return Math.hypot(v.x, v.y)
}

function format(value: number): string {
  // 控制小数位，避免 SVG path 过长；0.01 在 flow 坐标下不可见。
  return (Math.round(value * 100) / 100).toString()
}

/** 两个向量（同一起点）的夹角，范围 [0, π]。 */
function interiorAngle(a: EdgePoint, b: EdgePoint): number {
  const la = length(a)
  const lb = length(b)
  if (la === 0 || lb === 0) return Math.PI
  const cos = Math.min(1, Math.max(-1, (a.x * b.x + a.y * b.y) / (la * lb)))
  return Math.acos(cos)
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
}

/** 将折线除首尾外的折点按角度做二次贝塞尔圆角，返回 SVG path。 */
export function roundedPolyline(points: readonly EdgePoint[]): string {
  if (points.length < 2) return ''
  let d = `M ${format(points[0].x)} ${format(points[0].y)}`
  for (let index = 1; index < points.length - 1; index++) {
    const previous = points[index - 1]
    const current = points[index]
    const next = points[index + 1]
    const incoming = subtract(previous, current)
    const outgoing = subtract(next, current)
    const inLength = length(incoming)
    const outLength = length(outgoing)
    if (inLength < 0.01 || outLength < 0.01) continue
    const angle = interiorAngle(incoming, outgoing)
    const sharpness = clamp01((CORNER_START - angle) / (CORNER_START - CORNER_MIN))
    const radius = Math.min(inLength, outLength) * MAX_RADIUS_RATIO * sharpness
    if (radius < 0.5) {
      d += ` L ${format(current.x)} ${format(current.y)}`
      continue
    }
    const entry = {
      x: current.x + (incoming.x / inLength) * radius,
      y: current.y + (incoming.y / inLength) * radius,
    }
    const exit = {
      x: current.x + (outgoing.x / outLength) * radius,
      y: current.y + (outgoing.y / outLength) * radius,
    }
    d += ` L ${format(entry.x)} ${format(entry.y)}`
    d += ` Q ${format(current.x)} ${format(current.y)} ${format(exit.x)} ${format(exit.y)}`
  }
  const last = points[points.length - 1]
  d += ` L ${format(last.x)} ${format(last.y)}`
  return d
}

/**
 * 由源/目标端口坐标生成三段式连线路径。
 *
 * 源端口在节点右侧、目标端口在节点左侧，因此两端的水平伸出段分别固定向右、向左，
 * 始终朝节点外侧延伸；即使目标在源节点左侧（回边）也不会伸进节点内部。
 */
export function shaderEdgePath(
  source: EdgePoint,
  target: EdgePoint,
  stub: number = EDGE_STUB,
): string {
  const sourceStub: EdgePoint = { x: source.x + stub, y: source.y }
  const targetStub: EdgePoint = { x: target.x - stub, y: target.y }
  return roundedPolyline([source, sourceStub, targetStub, target])
}
