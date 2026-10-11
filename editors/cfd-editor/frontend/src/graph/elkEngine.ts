import ELK from 'elkjs/lib/elk-api.js'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'

export type ElkLayoutEngine = InstanceType<typeof ELK>

let elk: ElkLayoutEngine | null = null

/** ELK 引擎单例：生命周期收敛于此，布局客户端只经 `getLayoutEngine` 获取。 */
export function getLayoutEngine(): ElkLayoutEngine {
  if (!elk) elk = new ELK({ workerUrl: elkWorkerUrl })
  return elk
}

export function terminateLayoutEngine(): void {
  elk?.terminateWorker()
  elk = null
}
