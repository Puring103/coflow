import type { ElkNode } from 'elkjs/lib/elk-api'
import type { LayoutWorkerRequest, LayoutWorkerResponse } from './GraphView.layout.worker'

const LAYOUT_WORKER_TIMEOUT_MS = 20_000

let layoutWorker: Worker | null = null
let nextLayoutRequestId = 1
const layoutRequests = new Map<number, {
  resolve: (positions: Map<string, { x: number; y: number }>) => void
  reject: (error: Error) => void
  timeout: number
}>()

export async function runGraphLayoutInWorker(
  graph: ElkNode,
): Promise<Map<string, { x: number; y: number }>> {
  const id = nextLayoutRequestId++
  const worker = getLayoutWorker()
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      resetLayoutWorker(new Error('Graph layout worker timed out'))
    }, LAYOUT_WORKER_TIMEOUT_MS)
    layoutRequests.set(id, { resolve, reject, timeout })
    try {
      worker.postMessage({ id, graph } satisfies LayoutWorkerRequest)
    } catch (error) {
      clearTimeout(timeout)
      layoutRequests.delete(id)
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}

function getLayoutWorker(): Worker {
  if (!layoutWorker) {
    layoutWorker = new Worker(new URL('./GraphView.layout.worker.ts', import.meta.url), { type: 'module' })
    layoutWorker.onmessage = (event: MessageEvent<LayoutWorkerResponse>) => {
      const response = event.data
      const pending = layoutRequests.get(response.id)
      if (!pending) return
      clearTimeout(pending.timeout)
      layoutRequests.delete(response.id)
      if (response.ok) pending.resolve(new Map(response.positions))
      else pending.reject(new Error(response.error))
    }
    layoutWorker.onerror = event => {
      resetLayoutWorker(new Error(event.message || 'Graph layout worker failed'))
    }
    layoutWorker.onmessageerror = () => {
      resetLayoutWorker(new Error('Graph layout worker returned an unreadable response'))
    }
  }
  return layoutWorker
}

function resetLayoutWorker(error: Error): void {
  layoutWorker?.terminate()
  layoutWorker = null
  for (const [id, pending] of layoutRequests) {
    clearTimeout(pending.timeout)
    pending.reject(error)
    layoutRequests.delete(id)
  }
}
