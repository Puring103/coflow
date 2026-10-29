import { createContext } from 'react'

/** 当前绘制层发布的端口集合；完整关系与行几何保持在节点数据中。 */
export const GraphPortWindowContext = createContext<ReadonlyMap<string, readonly string[]> | null>(null)
