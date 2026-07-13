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
    const oldRequest = deferred<string[]>()
    const getEnumVariants = vi.fn()
      .mockImplementationOnce(() => oldRequest.promise)
      .mockResolvedValueOnce(['new'])
    const lookups = new EditorLookupController(backend({ getEnumVariants }))

    lookups.adopt({ sessionId: 1, revision: 3 })
    const oldResult = lookups.loadEnumVariants('Quality')
    lookups.adopt({ sessionId: 2, revision: 0 })
    const newResult = await lookups.loadEnumVariants('Quality')
    oldRequest.resolve(['old'])

    expect(newResult).toEqual({ ok: true, value: ['new'] })
    expect(await oldResult).toEqual({ ok: false, reason: 'superseded' })
    expect(await lookups.loadEnumVariants('Quality')).toEqual({ ok: true, value: ['new'] })
    expect(getEnumVariants).toHaveBeenCalledTimes(2)
  })

  it('deduplicates concurrent lookups inside one editor generation', async () => {
    const request = deferred<string[]>()
    const getEnumVariants = vi.fn(() => request.promise)
    const lookups = new EditorLookupController(backend({ getEnumVariants }))
    lookups.adopt({ sessionId: 7, revision: 4 })

    const first = lookups.loadEnumVariants('Quality')
    const second = lookups.loadEnumVariants('Quality')
    request.resolve(['Common', 'Rare'])

    await expect(first).resolves.toEqual({ ok: true, value: ['Common', 'Rare'] })
    await expect(second).resolves.toEqual({ ok: true, value: ['Common', 'Rare'] })
    expect(getEnumVariants).toHaveBeenCalledTimes(1)
  })
})
