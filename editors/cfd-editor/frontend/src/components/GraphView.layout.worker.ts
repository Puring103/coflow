/// <reference lib="webworker" />

import ELK from 'elkjs/lib/elk-api.js'
import type { ElkNode } from 'elkjs/lib/elk-api'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'

export interface LayoutWorkerRequest {
  id: number
  graph: ElkNode
}

export type LayoutWorkerResponse =
  | { id: number; ok: true; positions: [string, { x: number; y: number }][] }
  | { id: number; ok: false; error: string }

const ctx = self as DedicatedWorkerGlobalScope
const elk = new ELK({ workerUrl: elkWorkerUrl })

ctx.onmessage = async (event: MessageEvent<LayoutWorkerRequest>) => {
  const { id, graph } = event.data
  try {
    const laidOut = await elk.layout(graph)
    const children = laidOut.children ?? []
    const minX = children.length > 0 ? Math.min(...children.map(n => n.x ?? 0)) : 0
    const positions: [string, { x: number; y: number }][] = children.map(n => [
      n.id,
      { x: (n.x ?? 0) - minX, y: n.y ?? 0 },
    ])
    ctx.postMessage({ id, ok: true, positions } satisfies LayoutWorkerResponse)
  } catch (err) {
    ctx.postMessage({
      id,
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies LayoutWorkerResponse)
  }
}
