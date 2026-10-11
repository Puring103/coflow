/**
 * 节点拖动对齐参考线：采用 React Flow 官方 “Helper Lines” 示例的判定方式
 * （被拖节点的左右/上下边与附近节点边缘对齐），并额外加入“中心线对中心线”对齐。
 * 纯几何计算，独立于 React Flow，便于单测。
 */

export interface AlignBounds {
  left: number
  right: number
  top: number
  bottom: number
  width: number
  height: number
}

export interface HelperLines {
  /** 横向参考线的 flow y 坐标。 */
  horizontal?: number
  /** 竖向参考线的 flow x 坐标。 */
  vertical?: number
}

export interface HelperLinesResult extends HelperLines {
  /** 吸附后的绝对位置；undefined 表示该轴不吸附。 */
  snapPosition: { x?: number; y?: number }
}

export interface AlignableNode {
  position: { x: number; y: number }
  measured?: { width?: number; height?: number }
}

/** 由节点当前位置与测量尺寸得到对齐判定用的边界框。 */
export function boundsOf(node: AlignableNode): AlignBounds {
  const width = node.measured?.width ?? 280
  const height = node.measured?.height ?? 160
  return {
    left: node.position.x,
    right: node.position.x + width,
    top: node.position.y,
    bottom: node.position.y + height,
    width,
    height,
  }
}

/** 多个边界框的并集，用于整组拖动时作为单一吸附对象。 */
export function unionBounds(list: readonly AlignBounds[]): AlignBounds | null {
  if (list.length === 0) return null
  let left = Infinity
  let top = Infinity
  let right = -Infinity
  let bottom = -Infinity
  for (const bounds of list) {
    left = Math.min(left, bounds.left)
    top = Math.min(top, bounds.top)
    right = Math.max(right, bounds.right)
    bottom = Math.max(bottom, bounds.bottom)
  }
  return { left, top, right, bottom, width: right - left, height: bottom - top }
}

/**
 * 计算被拖动对象相对其他对象的对齐吸附。
 *
 * 判定顺序（与官方示例一致）覆盖左左、右右、左右、右左、上上、下上、下下、上下，
 * 并追加中心线对中心线；每个轴取距离最近的一条，且必须落在 distance 阈值内。
 */
export function getHelperLines(
  moving: AlignBounds,
  others: readonly AlignBounds[],
  distance = 5,
): HelperLinesResult {
  const result: HelperLinesResult = { snapPosition: {} }
  let horizontalDistance = distance
  let verticalDistance = distance
  const movingCenterX = moving.left + moving.width / 2
  const movingCenterY = moving.top + moving.height / 2
  for (const other of others) {
    const leftLeft = Math.abs(moving.left - other.left)
    if (leftLeft < verticalDistance) {
      result.snapPosition.x = other.left
      result.vertical = other.left
      verticalDistance = leftLeft
    }

    const rightRight = Math.abs(moving.right - other.right)
    if (rightRight < verticalDistance) {
      result.snapPosition.x = other.right - moving.width
      result.vertical = other.right
      verticalDistance = rightRight
    }

    const leftRight = Math.abs(moving.left - other.right)
    if (leftRight < verticalDistance) {
      result.snapPosition.x = other.right
      result.vertical = other.right
      verticalDistance = leftRight
    }

    const rightLeft = Math.abs(moving.right - other.left)
    if (rightLeft < verticalDistance) {
      result.snapPosition.x = other.left - moving.width
      result.vertical = other.left
      verticalDistance = rightLeft
    }

    // 中心线对中心线。
    const centerX = Math.abs(movingCenterX - (other.left + other.width / 2))
    if (centerX < verticalDistance) {
      const otherCenterX = other.left + other.width / 2
      result.snapPosition.x = otherCenterX - moving.width / 2
      result.vertical = otherCenterX
      verticalDistance = centerX
    }

    const topTop = Math.abs(moving.top - other.top)
    if (topTop < horizontalDistance) {
      result.snapPosition.y = other.top
      result.horizontal = other.top
      horizontalDistance = topTop
    }

    const bottomTop = Math.abs(moving.bottom - other.top)
    if (bottomTop < horizontalDistance) {
      result.snapPosition.y = other.top - moving.height
      result.horizontal = other.top
      horizontalDistance = bottomTop
    }

    const bottomBottom = Math.abs(moving.bottom - other.bottom)
    if (bottomBottom < horizontalDistance) {
      result.snapPosition.y = other.bottom - moving.height
      result.horizontal = other.bottom
      horizontalDistance = bottomBottom
    }

    const topBottom = Math.abs(moving.top - other.bottom)
    if (topBottom < horizontalDistance) {
      result.snapPosition.y = other.bottom
      result.horizontal = other.bottom
      horizontalDistance = topBottom
    }

    // 中心线对中心线。
    const centerY = Math.abs(movingCenterY - (other.top + other.height / 2))
    if (centerY < horizontalDistance) {
      const otherCenterY = other.top + other.height / 2
      result.snapPosition.y = otherCenterY - moving.height / 2
      result.horizontal = otherCenterY
      horizontalDistance = centerY
    }
  }
  return result
}
