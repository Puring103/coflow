import { describe, expect, it, vi } from 'vitest'
import { EditorLookupController, type EditorLookupBackend } from './editorLookups'
import type { FieldValue } from '../wire'

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

describe('EditorLookupController', () => {
  it('rejects an old editor generation response without caching it', async () => {
    const oldRequest = deferred<{ name: string, label: string | null, description: string | null }[]>()
    const getEnumVariants = vi.fn()
      .mockImplementationOnce(() => oldRequest.promise)
      .mockResolvedValueOnce([{ name: 'new', label: null, description: null }])
    const lookups = new EditorLookupController(backend({ getEnumVariants }))

    lookups.adopt({ sessionId: 1, revision: 3 })
    const oldResult = lookups.loadEnumVariants('Quality')
    lookups.adopt({ sessionId: 2, revision: 0 })
    const newResult = await lookups.loadEnumVariants('Quality')
    oldRequest.resolve([{ name: 'old', label: null, description: null }])

    expect(newResult).toEqual({ ok: true, value: [{ name: 'new', label: null, description: null }] })
    expect(await oldResult).toEqual({ ok: false, reason: 'superseded' })
    expect(await lookups.loadEnumVariants('Quality')).toEqual({ ok: true, value: [{ name: 'new', label: null, description: null }] })
    expect(getEnumVariants).toHaveBeenCalledTimes(2)
  })

  it('deduplicates concurrent lookups inside one editor generation', async () => {
    const request = deferred<{ name: string, label: string | null, description: string | null }[]>()
    const getEnumVariants = vi.fn(() => request.promise)
    const lookups = new EditorLookupController(backend({ getEnumVariants }))
    lookups.adopt({ sessionId: 7, revision: 4 })

    const first = lookups.loadEnumVariants('Quality')
    const second = lookups.loadEnumVariants('Quality')
    request.resolve([{ name: 'Common', label: null, description: null }, { name: 'Rare', label: null, description: null }])

    await expect(first).resolves.toEqual({ ok: true, value: [{ name: 'Common', label: null, description: null }, { name: 'Rare', label: null, description: null }] })
    await expect(second).resolves.toEqual({ ok: true, value: [{ name: 'Common', label: null, description: null }, { name: 'Rare', label: null, description: null }] })
    expect(getEnumVariants).toHaveBeenCalledTimes(1)
  })
})
