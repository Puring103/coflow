/** 每个视图保留一个运行任务和一个最新候选，不能终止其他视图共享的 worker。 */
export class SupersededTask extends Error {
  constructor() { super('Task superseded') }
}

interface Job<T> {
  version: number
  run: (isCurrent: () => boolean) => Promise<T>
  resolve: (value: T | undefined) => void
  reject: (error: unknown) => void
}

export class LatestTaskQueue<T> {
  private version = 0
  private running = false
  private pending: Job<T> | undefined

  schedule(run: Job<T>['run']): Promise<T | undefined> {
    const version = ++this.version
    this.pending?.resolve(undefined)
    return new Promise((resolve, reject) => {
      this.pending = { version, run, resolve, reject }
      void this.drain()
    })
  }

  cancel(): void {
    this.version++
    this.pending?.resolve(undefined)
    this.pending = undefined
  }

  private async drain(): Promise<void> {
    if (this.running) return
    this.running = true
    try {
      while (this.pending) {
        const job = this.pending
        this.pending = undefined
        const current = () => job.version === this.version
        try {
          const result = await job.run(current)
          job.resolve(current() ? result : undefined)
        } catch (error) {
          if (!current() || error instanceof SupersededTask) job.resolve(undefined)
          else job.reject(error)
        }
      }
    } finally { this.running = false }
  }
}
