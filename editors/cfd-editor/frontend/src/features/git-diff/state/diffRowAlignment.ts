export interface DiffRowMetrics { path: string; top: number; height: number }
export interface DiffRowSpacing { index: number; before: number; height: number }

/** 按两侧共有且顺序一致的路径补空白；缺失项保持空白，不伪造旧值或新值。 */
export function planDiffRowAlignment(left: readonly DiffRowMetrics[], right: readonly DiffRowMetrics[]) {
  const leftSpacing: DiffRowSpacing[] = []
  const rightSpacing: DiffRowSpacing[] = []
  const rightByPath = new Map(right.map((row, index) => [row.path, index]))
  let leftShift = 0
  let rightShift = 0
  let lastIndex = -1
  left.forEach((row, index) => {
    const rightIndex = rightByPath.get(row.path)
    if (rightIndex === undefined || rightIndex <= lastIndex) return
    lastIndex = rightIndex
    const other = right[rightIndex]
    const delta = other.top + rightShift - row.top - leftShift
    const leftBefore = Math.max(0, delta)
    const rightBefore = Math.max(0, -delta)
    const height = Math.max(row.height, other.height)
    leftSpacing.push({ index, before: leftBefore, height })
    rightSpacing.push({ index: rightIndex, before: rightBefore, height })
    leftShift += leftBefore + height - row.height
    rightShift += rightBefore + height - other.height
  })
  return { left: leftSpacing, right: rightSpacing }
}
