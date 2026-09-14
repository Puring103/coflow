import type { ElkNode } from 'elkjs/lib/elk-api'
import type { Position } from './metrics'

/** 图布局运行器接口：布局编排只依赖此类型，不知晓 ELK。 */
export type GraphLayoutRunner = (graph: ElkNode) => Promise<Map<string, Position>>
