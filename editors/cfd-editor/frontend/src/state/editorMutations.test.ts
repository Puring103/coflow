import { describe, expect, it, vi } from 'vitest'
import type { RecordRow } from '../bindings/RecordRow'
import type { FileRecords } from '../bindings/FileRecords'
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
  it('skips backend writes for deep value no-ops', async () => {
    const writeField = vi.fn()
    const port: EditorMutationPort = {
      currentGeneration: () => ({ sessionId: 1, revision: 1 }),
      publish: vi.fn(),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
      optimisticWriteField: vi.fn(() => ({
        changed: false,
        row,
        reapply: vi.fn(),
        rollback: vi.fn(),
      })),
    }
    const mutations = new EditorMutationController(
      backend(writeField),
      port,
      new MutationHistoryController(),
    )

    await expect(mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'old' },
    )).resolves.toBe(row)

    expect(writeField).not.toHaveBeenCalled()
    expect(port.publish).not.toHaveBeenCalled()
  })

  it('publishes a cached row projection without requiring a full file reload', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const projected = { revision: 2, file_path: 'data/items.cfd' } as FileRecords
    const fileRecordsForRow = vi.fn(() => projected)
    const publish = vi.fn(async request => {
      generation = { sessionId: request.sessionId, revision: request.revision }
      return committed(undefined)
    })
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish,
      fileRecordsForRow,
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
    }
    const mutations = new EditorMutationController(
      backend(vi.fn(async () => writeOutcome(2, 'old', 'new'))),
      port,
      new MutationHistoryController(),
    )

    await mutations.writeField('data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'new' })

    expect(fileRecordsForRow).toHaveBeenCalledWith('data/items.cfd', coordinate, row, 2)
    expect(publish.mock.calls[0][0].knownRecords).toBe(projected)
    expect(publish.mock.calls[0][0].topologyChanged).toBe(false)
  })

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
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(1))
    const newer = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'three' },
    )
    revision2.resolve(writeOutcome(2, 'old', 'two'))
    await older
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(2))
    revision3.resolve(writeOutcome(3, 'two', 'three'))
    await newer

    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([2, 3])
  })

  it('coalesces adjacent queued writes to the same field', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const firstOutcome = deferred<WriteFieldOutcome>()
    const latestOutcome = deferred<WriteFieldOutcome>()
    const writeField = vi.fn()
      .mockImplementationOnce(() => firstOutcome.promise)
      .mockImplementationOnce(() => latestOutcome.promise)
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

    const first = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'one' },
    )
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(1))
    const skipped = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'two' },
    )
    const latest = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'three' },
    )
    expect(writeField).toHaveBeenCalledTimes(1)

    firstOutcome.resolve(writeOutcome(2, 'old', 'one'))
    await first
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(2))
    expect(writeField).toHaveBeenLastCalledWith(
      1,
      coordinate,
      fieldPath,
      { kind: 'string', value: 'three' },
    )
    latestOutcome.resolve(writeOutcome(3, 'one', 'three'))
    await Promise.all([skipped, latest])

    expect(history.getSnapshot().undo.map(entry => entry.revision)).toEqual([2, 3])
  })

  it('reapplies queued optimistic writes before advancing the mutation queue', async () => {
    let generation = { sessionId: 1, revision: 1 }
    const firstOutcome = deferred<WriteFieldOutcome>()
    const secondOutcome = deferred<WriteFieldOutcome>()
    const writeField = vi.fn()
      .mockImplementationOnce(() => firstOutcome.promise)
      .mockImplementationOnce(() => secondOutcome.promise)
    const firstProjection = { changed: true, reapply: vi.fn(), rollback: vi.fn() }
    const secondProjection = { changed: true, reapply: vi.fn(), rollback: vi.fn() }
    const optimisticWriteField = vi.fn()
      .mockReturnValueOnce(firstProjection)
      .mockReturnValueOnce(secondProjection)
    const port: EditorMutationPort = {
      currentGeneration: () => generation,
      publish: vi.fn(async request => {
        generation = { sessionId: request.sessionId, revision: request.revision }
        return committed(undefined)
      }),
      rebindCoordinate: vi.fn(),
      recoverPublication: vi.fn(() => false),
      reportError: vi.fn(),
      optimisticWriteField,
    }
    const mutations = new EditorMutationController(
      backend(writeField),
      port,
      new MutationHistoryController(),
    )

    const first = mutations.writeField(
      'data/items.cfd',
      coordinate,
      [{ kind: 'field', value: 'name' }],
      { kind: 'string', value: 'first' },
    )
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(1))
    const second = mutations.writeField(
      'data/items.cfd',
      coordinate,
      [{ kind: 'field', value: 'description' }],
      { kind: 'string', value: 'second' },
    )

    firstOutcome.resolve(writeOutcome(2, 'old', 'first'))
    await first
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(2))
    expect(secondProjection.reapply).toHaveBeenCalledTimes(1)
    secondOutcome.resolve(writeOutcome(3, 'old', 'second'))
    await second
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
    await vi.waitFor(() => expect(writeField).toHaveBeenCalledTimes(1))
    const queued = mutations.writeField(
      'data/items.cfd', coordinate, fieldPath, { kind: 'string', value: 'stale' },
    )

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
