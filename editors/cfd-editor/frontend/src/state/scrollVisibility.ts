export interface AxisRange {
  start: number
  end: number
}

/** Return the smallest scroll delta that fully reveals a target range.
 * Oversized targets align their leading edge because both edges cannot fit. */
export function visibilityScrollDelta(viewport: AxisRange, target: AxisRange): number {
  const viewportSize = Math.max(0, viewport.end - viewport.start)
  const targetSize = Math.max(0, target.end - target.start)
  if (targetSize > viewportSize) {
    return target.start === viewport.start ? 0 : target.start - viewport.start
  }
  if (target.start < viewport.start) return target.start - viewport.start
  if (target.end > viewport.end) return target.end - viewport.end
  return 0
}
