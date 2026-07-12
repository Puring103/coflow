import { describe, expect, it } from 'vitest'
import {
  committed,
  failed,
  MutationHistoryController,
  ProjectGenerationController,
  superseded,
  type EditEntry,
} from './editorState'

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
