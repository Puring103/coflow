import { describe, expect, it } from 'vitest'
import { registerDataEdit, flushDataEdits, deferDataNavigation } from './editSession'

describe('data edit navigation barrier', () => {
  it('keeps navigation blocked on save failure', async () => {
    const unregister = registerDataEdit(async () => false, () => true)
    let navigated = false
    expect(deferDataNavigation(() => { navigated = true })).toBe(true)
    expect(await flushDataEdits()).toBe(false)
    await Promise.resolve()
    expect(navigated).toBe(false)
    unregister()
  })
  it('waits for all editors and only executes the most recent navigation request', async () => {
    let resolve!: (ok: boolean) => void
    const pending = new Promise<boolean>(done => { resolve = done })
    const unregister = registerDataEdit(() => pending, () => true)
    const calls: number[] = []
    deferDataNavigation(() => calls.push(1))
    deferDataNavigation(() => calls.push(2))
    expect(calls).toEqual([])
    resolve(true)
    await flushDataEdits()
    await Promise.resolve()
    expect(calls).toEqual([2])
    unregister()
    expect(deferDataNavigation(() => calls.push(3))).toBe(false)
  })
})
