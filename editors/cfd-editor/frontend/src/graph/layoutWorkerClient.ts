import type { ElkNode } from 'elkjs/lib/elk-api'
import { getLayoutEngine, terminateLayoutEngine } from './elkEngine'
import type { Position } from './metrics'

const LAYOUT_WORKER_TIMEOUT_MS = 20_000

let nextLayoutRequestId = 1
const layoutRequests = new Map<number, {
  reject: (error: Error) => void
  timeout: number
}>()

function failAllPending(error: Error): void {
  terminateLayoutEngine()
  for (const [id, pending] of layoutRequests) {
    clearTimeout(pending.timeout)
    pending.reject(error)
    layoutRequests.delete(id)
  }
}

/** Worker 布局客户端：请求表 + 超时 + 坐标归一收敛于此。 */
export async function runGraphLayoutInWorker(
  graph: ElkNode,
): Promise<Map<string, Position>> {
  const id = nextLayoutRequestId++
  const engine = getLayoutEngine()
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      failAllPending(new Error('Graph layout worker timed out'))
    }, LAYOUT_WORKER_TIMEOUT_MS)
    layoutRequests.set(id, { reject, timeout })
    engine.layout(graph).then((laidOut: ElkNode) => {
      const pending = layoutRequests.get(id)
      if (!pending) return
      clearTimeout(pending.timeout)
      layoutRequests.delete(id)
      const children = laidOut.children ?? []
      const minX = children.length > 0 ? Math.min(...children.map(node => node.x ?? 0)) : 0
      resolve(new Map(children.map(node => [
        node.id,
        { x: (node.x ?? 0) - minX, y: node.y ?? 0 },
      ])))
    }).catch((error: unknown) => {
      const pending = layoutRequests.get(id)
      if (!pending) return
      clearTimeout(timeout)
      layoutRequests.delete(id)
      reject(error instanceof Error ? error : new Error(String(error)))
    })
  })
}
