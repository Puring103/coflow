export interface Point {
  x: number
  y: number
}

export interface Size {
  width: number
  height: number
}

export function fitViewportPosition(
  anchor: Point,
  floatingSize: Size,
  viewportSize: Size,
  padding = 8,
): Point {
  const maxX = Math.max(padding, viewportSize.width - floatingSize.width - padding)
  const maxY = Math.max(padding, viewportSize.height - floatingSize.height - padding)
  return {
    x: Math.min(Math.max(padding, anchor.x), maxX),
    y: Math.min(Math.max(padding, anchor.y), maxY),
  }
}
