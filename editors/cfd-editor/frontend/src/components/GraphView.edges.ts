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

export interface EdgeClipRect { left: number; top: number; right: number; bottom: number }
interface EdgeSegment { from: EdgePoint; to: EdgePoint; control?: EdgePoint }

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
function roundedSegments(points: readonly EdgePoint[]): EdgeSegment[] {
  if (points.length < 2) return []
  const segments: EdgeSegment[] = []
  let cursor = points[0]
  const line = (to: EdgePoint) => { segments.push({ from: cursor, to }); cursor = to }
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
      line(current)
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
    line(entry)
    segments.push({ from: entry, control: current, to: exit })
    cursor = exit
  }
  const last = points[points.length - 1]
  line(last)
  return segments
}

function segmentPath(segment: EdgeSegment): string {
  return segment.control
    ? `Q ${format(segment.control.x)} ${format(segment.control.y)} ${format(segment.to.x)} ${format(segment.to.y)}`
    : `L ${format(segment.to.x)} ${format(segment.to.y)}`
}

export function roundedPolyline(points: readonly EdgePoint[]): string {
  if (points.length < 2) return ''
  return `M ${format(points[0].x)} ${format(points[0].y)} ` + roundedSegments(points).map(segmentPath).join(' ')
}

/** Liang–Barsky 裁剪直线；圆角保持原曲线，仅剔除整个控制包围盒在屏外的段。 */
function clipSegment(segment: EdgeSegment, rect: EdgeClipRect): EdgeSegment | null {
  const { from, to, control } = segment
  if (control) return Math.max(from.x, to.x, control.x) < rect.left || Math.min(from.x, to.x, control.x) > rect.right
    || Math.max(from.y, to.y, control.y) < rect.top || Math.min(from.y, to.y, control.y) > rect.bottom ? null : segment
  const dx = to.x - from.x, dy = to.y - from.y
  let first = 0, last = 1
  for (const [p, q] of [[-dx, from.x - rect.left], [dx, rect.right - from.x], [-dy, from.y - rect.top], [dy, rect.bottom - from.y]]) {
    if (p === 0) { if (q < 0) return null; continue }
    const ratio = q / p
    if (p < 0) first = Math.max(first, ratio)
    else last = Math.min(last, ratio)
    if (first > last) return null
  }
  return { from: { x: from.x + dx * first, y: from.y + dy * first }, to: { x: from.x + dx * last, y: from.y + dy * last } }
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
  clip?: EdgeClipRect,
): string {
  const sourceStub: EdgePoint = { x: source.x + stub, y: source.y }
  const targetStub: EdgePoint = { x: target.x - stub, y: target.y }
  const points = [source, sourceStub, targetStub, target]
  if (!clip) return roundedPolyline(points)
  // 只缩短可见绘制路径，原始几何继续用于命中和重连。屏外长线的滤镜表面因此有界。
  let path = '', last: EdgePoint | undefined
  for (const segment of roundedSegments(points)) {
    const clipped = clipSegment(segment, clip)
    if (!clipped) continue
    if (!last || Math.abs(last.x - clipped.from.x) > 0.005 || Math.abs(last.y - clipped.from.y) > 0.005) {
      path += ` M ${format(clipped.from.x)} ${format(clipped.from.y)}`
    }
    path += ` ${segmentPath(clipped)}`
    last = clipped.to
  }
  return path.trim()
}
