/** 图布局度量唯一来源：布局层只依赖此接口，不知晓卡片实现。 */
export const NODE_WIDTH = 280
export const COLUMN_GAP = 280
export const ROW_GAP = 90
export const COMPONENT_GAP = 120
export const COMPACT_ZOOM_THRESHOLD = 0.65
export const HEADER_HEIGHT = 42
export const ROW_HEIGHT = 22
export const EDITABLE_ROW_HEIGHT = 34
export const MORE_BUTTON_HEIGHT = 28
export const VERTICAL_PADDING = 12

export type Position = { x: number; y: number }

/** 节点度量器：由卡片几何实现，布局层只依赖此接口。 */
export interface NodeMeasurer {
  measure(nodeId: string): number | undefined
}
