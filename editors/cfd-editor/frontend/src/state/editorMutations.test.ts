import { describe, expect, it, vi } from 'vitest'
import type { RecordRow } from '../bindings/RecordRow'
import type { WriteFieldOutcome } from '../bindings/WriteFieldOutcome'
import { committed, MutationHistoryController, superseded } from './editorState'
import {
  EditorMutationController,
  type EditorMutationBackend,
  type EditorMutationPort,
} from './editorMutations'

const coordinate = { actual_type: 'Item', key: 'sword' }
const fieldPath = [{ kind: 'field' as const, value: 'name' }]
const row = { coordinate } as RecordRow

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

function writeOutcome(revision: number, oldValue: string, newValue: string): WriteFieldOutcome {
  return {
    revision,
    row,
    diagnostics: [],
    old_value: { kind: 'string', value: oldValue },
    new_value: { kind: 'string', value: newValue },
    affected_files: ['data/items.cfd'],
    renamed: null,
  }
}

function backend(writeField: EditorMutationBackend['writeField']): EditorMutationBackend {
  return {
    writeField,
    editCollection: vi.fn(),
    renameRecordKey: vi.fn(),
    insertRecord: vi.fn(),
    deleteRecord: vi.fn(),
  }
}

describe('EditorMutationController', () => {
  it('records and replays history only through committed editor generations', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const writeField = vi.fn()
      .mockResolvedValueOnce(writeOutcome(2, 'old', 'new'))
      .mockResolvedValueOnce(writeOutcome(3, 'new', 'old'))
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async request => {
        generation = { sessionId: request.sessionId, revision: request.revision }
        return committed(undefined)
      }),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const history = new MutationHistoryController()
    const mutations = new EditorMutationController(backend(writeField), port, history)

    await expect(mutations.writeField(
      'data/items.cfd',
      coordinate,
      fieldPath,
      { kind: 'string', value: 'new' },
    )).resolves.toBe(row)
    expect(history.getSnapshot().undo).toHaveLength(1)

    await mutations.undo()

    expect(writeField).toHaveBeenNthCalledWith(
      2,
      1,
      coordinate,
      fieldPath,
      { kind: 'string', value: 'old' },
    )
    expect(history.getSnapshot().undo).toHaveLength(0)
    expect(history.getSnapshot().redo).toHaveLength(1)
  })

  it('records a committed outcome superseded without a history reset', async () => {
    const history = new MutationHistoryController()
    const port: EditorMutationPort = {
      currentGeneration: () => ({ sessionId: 1, revision: 1 }),
      publish: vi.fn(async () => superseded()),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const mutations = new EditorMutationController(
      backend(vi.fn(async () => writeOutcome(2, 'old', 'new'))),
      port,
      history,
    )

    await expect(mutations.writeField(
      'data/items.cfd',
      coordinate,
      fieldPath,
      { kind: 'string', value: 'new' },
    )).resolves.toBe(row)
    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([2])
  })

  it('serializes concurrent edits and keeps backend revision order', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const revision2 = deferred<WriteFieldOutcome>()
    const revision3 = deferred<WriteFieldOutcome>()
    const writeField = vi.fn()
      .mockImplementationOnce(() => revision2.promise)
      .mockImplementationOnce(() => revision3.promise)
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async request => {
        if (request.revision < generation.revision) return superseded()
        generation = { sessionId: request.sessionId, revision: request.revision }
        return committed(undefined)
      }),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const history = new MutationHistoryController()
    const mutations = new EditorMutationController(backend(writeField), port, history)

    const older = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'two' },
    )
    const newer = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'three' },
    )
    await Promise.resolve()
    expect(writeField).toHaveBeenCalledTimes(1)
    revision2.resolve(writeOutcome(2, 'old', 'two'))
    await older
    await Promise.resolve()
    expect(writeField).toHaveBeenCalledTimes(2)
    revision3.resolve(writeOutcome(3, 'two', 'three'))
    await newer

    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([2, 3])
  })

  it('keeps committed history when projection refresh falls back to reload', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async () => { throw new Error('refresh failed') }),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(request => {
        generation = { sessionId: request.sessionId, revision: request.revision }
        return true
      }),
      reportError: vi.fn(),
    }
    const history = new MutationHistoryController()
    const mutations = new EditorMutationController(
      backend(vi.fn(async () => writeOutcome(2, 'old', 'new'))),
      port,
      history,
    )

    await mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'new' },
    )

    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([2])
    expect(port.reportError).toHaveBeenCalledWith(
      1, '编辑已保存，但刷新失败', expect.any(Error), 2,
    )
  })

  it('does not restore stale history after an external generation reset', async () => {
    const pending = deferred<WriteFieldOutcome>()
    const history = new MutationHistoryController()
    const port: EditorMutationPort = {
      currentGeneration: () => ({ sessionId: 1, revision: 3 }),
      publish: vi.fn(async () => superseded()),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const mutations = new EditorMutationController(backend(vi.fn(() => pending.promise)), port, history)

    const mutation = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'new' },
    )
    await Promise.resolve()
    history.clear()
    pending.resolve(writeOutcome(2, 'old', 'new'))
    await mutation

    expect(history.getSnapshot().undo).toHaveLength(0)
    expect(port.rebindCoordinate).not.toHaveBeenCalled()
  })

  it('drops queued edits that belong to an externally replaced generation', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const pending = deferred<WriteFieldOutcome>()
    const writeField = vi.fn(() => pending.promise)
    const history = new MutationHistoryController()
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async () => superseded()),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const mutations = new EditorMutationController(backend(writeField), port, history)

    const inFlight = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'two' },
    )
    const queued = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'stale' },
    )
    await Promise.resolve()
    expect(writeField).toHaveBeenCalledTimes(1)

    generation = { sessionId: 1, revision: 10 }
    history.clear()
    pending.resolve(writeOutcome(2, 'old', 'two'))
    await Promise.all([inFlight, queued])

    expect(writeField).toHaveBeenCalledTimes(1)
    expect(history.getSnapshot().undo).toHaveLength(0)
  })

  it('serializes ordinary edits behind an in-flight undo', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const undoRequest = deferred<WriteFieldOutcome>()
    const editRequest = deferred<WriteFieldOutcome>()
    const writeField = vi.fn()
      .mockImplementationOnce(() => undoRequest.promise)
      .mockImplementationOnce(() => editRequest.promise)
    const history = new MutationHistoryController()
    history.record({
      kind: 'field',
      revision: 1,
      filePath: 'data/items.cfd',
      coordinate,
      fieldPath,
      oldValue: { kind: 'string', value: 'old' },
      newValue: { kind: 'string', value: 'new' },
    })
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async request => {
        generation = { sessionId: request.sessionId, revision: request.revision }
        return committed(undefined)
      }),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const mutations = new EditorMutationController(backend(writeField), port, history)

    const undo = mutations.undo()
    await Promise.resolve()
    const edit = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'latest' },
    )
    expect(writeField).toHaveBeenCalledTimes(1)

    undoRequest.resolve(writeOutcome(2, 'new', 'old'))
    await undo
    await Promise.resolve()
    expect(writeField).toHaveBeenCalledTimes(2)
    editRequest.resolve(writeOutcome(3, 'old', 'latest'))
    await edit

    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([3])
    expect(history.getSnapshot().redo).toHaveLength(0)
  })
})
