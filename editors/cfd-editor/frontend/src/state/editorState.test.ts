import { describe, expect, it } from 'vitest'
import {
  committed,
  failed,
  MutationHistoryController,
  publishMutationGeneration,
  ProjectGenerationController,
  superseded,
  type EditEntry,
  type MutationPublicationPort,
} from './editorState'
import type { FileRecords } from '../bindings/FileRecords'

const fieldEntry = (key: string): EditEntry => ({
  kind: 'field',
  filePath: 'data/items.cfd',
  coordinate: { actual_type: 'Item', key },
  fieldPath: [{ kind: 'field', value: 'name' }],
  oldValue: { kind: 'string', value: 'old' },
  newValue: { kind: 'string', value: 'new' },
})

describe('ProjectGenerationController', () => {
  it('accepts only newer snapshots from the adopted session', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 2, revision: 3 })

    expect(generation.acceptSnapshot({ session_id: 1, revision: 99 })).toBe(false)
    expect(generation.acceptSnapshot({ session_id: 2, revision: 3 })).toBe(false)
    expect(generation.acceptSnapshot({ session_id: 2, revision: 4 })).toBe(true)
    expect(generation.isCurrent(2, 4)).toBe(true)
  })

  it('rejects mutation outcomes from old sessions and revisions', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 7, revision: 10 })

    expect(generation.acceptMutation(6, 20)).toBe(false)
    expect(generation.acceptMutation(7, 9)).toBe(false)
    expect(generation.acceptMutation(7, 11)).toBe(true)
  })

  it('invalidates stale request handlers when a newer project is adopted', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 1, revision: 0 })
    const staleRequest = generation.captureRequest()

    generation.adopt({ session_id: 2, revision: 0 })

    expect(generation.isRequestCurrent(staleRequest)).toBe(false)
  })

  it('lets the latest project request own success, error, and finalizer handlers', () => {
    const generation = new ProjectGenerationController()
    const first = generation.beginProjectRequest()
    const second = generation.beginProjectRequest()

    expect(generation.isProjectRequestCurrent(first)).toBe(false)
    expect(generation.isProjectRequestCurrent(second)).toBe(true)
  })

  it('does not invalidate current-generation requests while a project picker is open', () => {
    const generation = new ProjectGenerationController()
    generation.adopt({ session_id: 1, revision: 2 })
    const currentGenerationRequest = generation.captureRequest()

    generation.beginProjectRequest()

    expect(generation.isRequestCurrent(currentGenerationRequest)).toBe(true)
  })
})

const fileRecords = (revision: number, filePath = 'data/items.cfd') => ({
  revision,
  file_path: filePath,
}) as FileRecords

describe('publishMutationGeneration', () => {
  it('publishes the backend-newer revision when caller completion order is reversed', async () => {
    let currentRevision = 0
    let resolveOldRead: ((records: FileRecords) => void) | undefined
    const published: (readonly (readonly [string, FileRecords])[])[] = []
    let graphInvalidations = 0
    const port: MutationPublicationPort = {
      acceptRevision: (_sessionId, revision) => {
        if (revision < currentRevision) return false
        currentRevision = revision
        return true
      },
      isCurrent: (_sessionId, revision) => currentRevision === revision,
      getFileRecords: async (_sessionId, filePath) => new Promise(resolve => {
        resolveOldRead = records => resolve(records.file_path === filePath ? records : fileRecords(1, filePath))
      }),
      publishFileRecords: records => published.push(records),
      invalidateGraphs: () => { graphInvalidations += 1 },
    }

    const older = publishMutationGeneration(port, {
      sessionId: 1,
      revision: 1,
      diagnostics: [],
      affectedFiles: ['data/items.cfd'],
      fallbackFile: 'data/items.cfd',
    })
    const newerRecords = fileRecords(2)
    const newer = await publishMutationGeneration(port, {
      sessionId: 1,
      revision: 2,
      diagnostics: [],
      affectedFiles: ['data/items.cfd'],
      fallbackFile: 'data/items.cfd',
      knownRecords: newerRecords,
    })
    resolveOldRead?.(fileRecords(1))

    expect(newer.status).toBe('committed')
    expect((await older).status).toBe('superseded')
    expect(published).toEqual([[['data/items.cfd', newerRecords]]])
    expect(graphInvalidations).toBe(1)
  })
})

describe('MutationHistoryController', () => {
  it('moves history only after a committed undo', async () => {
    const history = new MutationHistoryController()
    history.record(fieldEntry('sword'))

    expect((await history.undo(async () => failed())).status).toBe('failed')
    expect(history.getSnapshot().undo).toHaveLength(1)
    expect(history.getSnapshot().redo).toHaveLength(0)

    expect((await history.undo(async () => committed(undefined))).status).toBe('committed')
    expect(history.getSnapshot().undo).toHaveLength(0)
    expect(history.getSnapshot().redo).toHaveLength(1)
  })

  it('serializes history operations', async () => {
    const history = new MutationHistoryController()
    history.record(fieldEntry('shield'))
    let release: (() => void) | undefined
    const first = history.undo(() => new Promise(resolve => {
      release = () => resolve(committed(undefined))
    }))

    expect(history.getSnapshot().busy).toBe(true)
    expect((await history.undo(async () => committed(undefined))).status).toBe('superseded')
    release?.()
    await first
    expect(history.getSnapshot().redo).toHaveLength(1)
  })

  it('does not move history for superseded redo', async () => {
    const history = new MutationHistoryController()
    history.record(fieldEntry('potion'))
    await history.undo(async () => committed(undefined))

    expect((await history.redo(async () => superseded())).status).toBe('superseded')
    expect(history.getSnapshot().undo).toHaveLength(0)
    expect(history.getSnapshot().redo).toHaveLength(1)
  })
})
