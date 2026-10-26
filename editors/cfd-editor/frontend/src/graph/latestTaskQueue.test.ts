import { expect, it } from 'vitest'
import { LatestTaskQueue, SupersededTask } from './latestTaskQueue'

function barrier() {
  let release!: () => void
  return { wait: new Promise<void>(resolve => { release = resolve }), release: () => release() }
}

it('runs the current job and only the latest waiting job, settling every promise', async () => {
  const queue = new LatestTaskQueue<number>(), gate = barrier(), ran: number[] = []
  const first = queue.schedule(async current => { ran.push(1); await gate.wait; if (!current()) throw new SupersededTask(); return 1 })
  const second = queue.schedule(async () => { ran.push(2); return 2 })
  const third = queue.schedule(async () => { ran.push(3); return 3 })
  expect(await second).toBeUndefined()
  gate.release()
  expect(await Promise.all([first, third])).toEqual([undefined, 3])
  expect(ran).toEqual([1, 3])
})

it('cancel settles waiting jobs without interrupting another view and allows reuse after cleanup', async () => {
  const one = new LatestTaskQueue<number>(), two = new LatestTaskQueue<number>(), gate = barrier()
  const first = one.schedule(async () => { await gate.wait; return 1 })
  const pending = one.schedule(async () => 2)
  const other = two.schedule(async () => 3)
  one.cancel()
  expect(await pending).toBeUndefined()
  expect(await other).toBe(3)
  const latest = one.schedule(async () => 4)
  gate.release()
  expect(await Promise.all([first, latest])).toEqual([undefined, 4])
})

it('reports a current failure and continues to accept work', async () => {
  const queue = new LatestTaskQueue<number>()
  await expect(queue.schedule(async () => { throw new Error('worker failed') })).rejects.toThrow('worker failed')
  expect(await queue.schedule(async () => 5)).toBe(5)
})
