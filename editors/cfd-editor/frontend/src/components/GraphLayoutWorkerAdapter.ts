import ELK from 'elkjs/lib/elk-api.js'
import type { ElkNode } from 'elkjs/lib/elk-api'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'

const LAYOUT_WORKER_TIMEOUT_MS = 20_000

type ElkLayoutEngine = InstanceType<typeof ELK>

let nextLayoutRequestId = 1
const layoutRequests = new Map<number, {
  reject: (error: Error) => void
  timeout: number
}>()
let elk: ElkLayoutEngine | null = null

export async function runGraphLayoutInWorker(
  graph: ElkNode,
): Promise<Map<string, { x: number; y: number }>> {
  const id = nextLayoutRequestId++
  const engine = getLayoutEngine()
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      resetLayoutEngine(new Error('Graph layout worker timed out'))
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

function getLayoutEngine(): ElkLayoutEngine {
  if (!elk) elk = new ELK({ workerUrl: elkWorkerUrl })
  return elk
}

function resetLayoutEngine(error: Error): void {
  elk?.terminateWorker()
  elk = null
  for (const [id, pending] of layoutRequests) {
    clearTimeout(pending.timeout)
    pending.reject(error)
    layoutRequests.delete(id)
  }
}
