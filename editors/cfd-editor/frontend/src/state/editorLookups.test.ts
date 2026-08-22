import { describe, expect, it, vi } from 'vitest'
import { EditorLookupController, type EditorLookupBackend } from './editorLookups'
import type { FieldValue } from '../wire'
import type { RefTarget } from '../bindings/RefTarget'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

function backend(overrides: Partial<EditorLookupBackend> = {}): EditorLookupBackend {
  return {
    getEnumVariants: vi.fn(async () => []),
    getRefTargets: vi.fn(async () => []),
    makeDefaultObject: vi.fn(async () => ({ kind: 'null' } as FieldValue)),
    createRecordDraft: vi.fn(async actualType => ({ actual_type: actualType, fields: [] })),
    ...overrides,
  }
}

function refTarget(key: string): RefTarget {
  return {
    coordinate: { actual_type: 'Item', key },
    file_path: 'data/items.cfd',
  }
}

describe('EditorLookupController', () => {
  it('rejects an old editor generation response without caching it', async () => {
    const oldRequest = deferred<{ name: string, value: bigint, label: string | null, description: string | null }[]>()
    const getEnumVariants = vi.fn()
      .mockImplementationOnce(() => oldRequest.promise)
      .mockResolvedValueOnce([{ name: 'new', value: 2n, label: null, description: null }])
    const lookups = new EditorLookupController(backend({ getEnumVariants }))

    lookups.adopt({ sessionId: 1, revision: 3 })
    const oldResult = lookups.loadEnumVariants('Quality')
    lookups.adopt({ sessionId: 2, revision: 0 })
    const newResult = await lookups.loadEnumVariants('Quality')
    oldRequest.resolve([{ name: 'old', value: 1n, label: null, description: null }])

    expect(newResult).toEqual({ ok: true, value: [{ name: 'new', value: 2n, label: null, description: null }] })
    expect(await oldResult).toEqual({ ok: false, reason: 'superseded' })
    expect(await lookups.loadEnumVariants('Quality')).toEqual({ ok: true, value: [{ name: 'new', value: 2n, label: null, description: null }] })
    expect(getEnumVariants).toHaveBeenCalledTimes(2)
  })

  it('deduplicates concurrent lookups inside one editor generation', async () => {
    const request = deferred<{ name: string, value: bigint, label: string | null, description: string | null }[]>()
    const getEnumVariants = vi.fn(() => request.promise)
    const lookups = new EditorLookupController(backend({ getEnumVariants }))
    lookups.adopt({ sessionId: 7, revision: 4 })

    const first = lookups.loadEnumVariants('Quality')
    const second = lookups.loadEnumVariants('Quality')
    request.resolve([{ name: 'Common', value: 0n, label: null, description: null }, { name: 'Rare', value: 1n, label: null, description: null }])

    await expect(first).resolves.toEqual({ ok: true, value: [{ name: 'Common', value: 0n, label: null, description: null }, { name: 'Rare', value: 1n, label: null, description: null }] })
    await expect(second).resolves.toEqual({ ok: true, value: [{ name: 'Common', value: 0n, label: null, description: null }, { name: 'Rare', value: 1n, label: null, description: null }] })
    expect(getEnumVariants).toHaveBeenCalledTimes(1)
  })

  it('keeps schema-scoped enum values across revisions in one session', async () => {
    const variants = [{ name: 'Common', value: 0n, label: null, description: null }]
    const getEnumVariants = vi.fn(async () => variants)
    const lookups = new EditorLookupController(backend({ getEnumVariants }))

    lookups.adopt({ sessionId: 7, revision: 1 })
    await expect(lookups.loadEnumVariants('Quality')).resolves.toEqual({ ok: true, value: variants })
    lookups.adopt({ sessionId: 7, revision: 2 })

    expect(lookups.cachedEnumVariants('Quality')).toBe(variants)
    await expect(lookups.loadEnumVariants('Quality')).resolves.toEqual({ ok: true, value: variants })
    expect(getEnumVariants).toHaveBeenCalledTimes(1)
  })

  it('allows a pending schema lookup to finish across a data revision', async () => {
    const request = deferred<{ name: string, value: bigint, label: string | null, description: string | null }[]>()
    const getEnumVariants = vi.fn(() => request.promise)
    const lookups = new EditorLookupController(backend({ getEnumVariants }))
    const variants = [{ name: 'Rare', value: 1n, label: null, description: null }]

    lookups.adopt({ sessionId: 7, revision: 1 })
    const result = lookups.loadEnumVariants('Quality')
    lookups.adopt({ sessionId: 7, revision: 2 })
    request.resolve(variants)

    await expect(result).resolves.toEqual({ ok: true, value: variants })
    expect(lookups.cachedEnumVariants('Quality')).toBe(variants)
    expect(getEnumVariants).toHaveBeenCalledTimes(1)
  })

  it('keeps stale reference targets available while refreshing a new revision', async () => {
    const refreshed = deferred<RefTarget[]>()
    const oldTargets = [refTarget('old')]
    const newTargets = [refTarget('new')]
    const getRefTargets = vi.fn()
      .mockResolvedValueOnce(oldTargets)
      .mockImplementationOnce(() => refreshed.promise)
    const lookups = new EditorLookupController(backend({ getRefTargets }))

    lookups.adopt({ sessionId: 7, revision: 1 })
    await expect(lookups.loadRefTargets('Item')).resolves.toEqual({ ok: true, value: oldTargets })
    lookups.adopt({ sessionId: 7, revision: 2 })

    expect(lookups.cachedRefTargets('Item')).toBe(oldTargets)
    const refresh = lookups.loadRefTargets('Item')
    expect(lookups.cachedRefTargets('Item')).toBe(oldTargets)
    refreshed.resolve(newTargets)

    await expect(refresh).resolves.toEqual({ ok: true, value: newTargets })
    expect(lookups.cachedRefTargets('Item')).toBe(newTargets)
    expect(getRefTargets).toHaveBeenCalledTimes(2)
  })

  it('rejects a reference response superseded by a newer revision', async () => {
    const oldRequest = deferred<RefTarget[]>()
    const getRefTargets = vi.fn()
      .mockImplementationOnce(() => oldRequest.promise)
      .mockResolvedValueOnce([refTarget('new')])
    const lookups = new EditorLookupController(backend({ getRefTargets }))

    lookups.adopt({ sessionId: 7, revision: 1 })
    const oldResult = lookups.loadRefTargets('Item')
    lookups.adopt({ sessionId: 7, revision: 2 })
    const newResult = lookups.loadRefTargets('Item')
    oldRequest.resolve([refTarget('old')])

    await expect(newResult).resolves.toEqual({ ok: true, value: [refTarget('new')] })
    await expect(oldResult).resolves.toEqual({ ok: false, reason: 'superseded' })
    expect(lookups.cachedRefTargets('Item')).toEqual([refTarget('new')])
  })

  it('retains stale reference targets after a failed refresh and retries later', async () => {
    const oldTargets = [refTarget('old')]
    const newTargets = [refTarget('new')]
    const getRefTargets = vi.fn()
      .mockResolvedValueOnce(oldTargets)
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(newTargets)
    const lookups = new EditorLookupController(backend({ getRefTargets }))

    lookups.adopt({ sessionId: 7, revision: 1 })
    await lookups.loadRefTargets('Item')
    lookups.adopt({ sessionId: 7, revision: 2 })

    await expect(lookups.loadRefTargets('Item')).resolves.toEqual({
      ok: false,
      reason: 'failed',
      error: 'temporary failure',
    })
    expect(lookups.cachedRefTargets('Item')).toBe(oldTargets)
    await expect(lookups.loadRefTargets('Item')).resolves.toEqual({ ok: true, value: newTargets })
    expect(lookups.cachedRefTargets('Item')).toBe(newTargets)
    expect(getRefTargets).toHaveBeenCalledTimes(3)
  })

  it('clears all cached values when the editor session changes', async () => {
    const lookups = new EditorLookupController(backend({
      getEnumVariants: vi.fn(async () => [{ name: 'Common', value: 0n, label: null, description: null }]),
      getRefTargets: vi.fn(async () => [refTarget('sword')]),
    }))

    lookups.adopt({ sessionId: 7, revision: 1 })
    await lookups.loadEnumVariants('Quality')
    await lookups.loadRefTargets('Item')
    lookups.adopt({ sessionId: 8, revision: 0 })

    expect(lookups.cachedEnumVariants('Quality')).toBeUndefined()
    expect(lookups.cachedRefTargets('Item')).toBeUndefined()
  })
})
