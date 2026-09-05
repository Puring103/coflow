export function resizedColumnWidth(
  startWidth: number,
  startX: number,
  clientX: number,
  zoom: number,
  minWidth: number,
): number {
  return Math.max(minWidth, startWidth + (clientX - startX) / zoom)
}

export function resizeScrollWidthFloor(
  scrollWidth: number,
  scrollLeft: number,
  clientWidth: number,
): number {
  return Math.max(scrollWidth, scrollLeft + clientWidth)
}

export function canReleaseResizeScrollFloor(
  tableWidth: number,
  scrollLeft: number,
  clientWidth: number,
): boolean {
  return scrollLeft + clientWidth <= tableWidth + 0.5
}
