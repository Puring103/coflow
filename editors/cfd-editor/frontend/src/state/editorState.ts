import type { ProjectSnapshot } from '../bindings/ProjectSnapshot'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { sameCoordinate, type FieldPathSegment, type FieldValue } from '../wire'

export type MutationResult<T = void> =
  | { status: 'committed'; value: T }
  | { status: 'superseded' }
  | { status: 'failed' }

export const committed = <T>(value: T): MutationResult<T> => ({ status: 'committed', value })
export const superseded = (): MutationResult<never> => ({ status: 'superseded' })
export const failed = (): MutationResult<never> => ({ status: 'failed' })

export class ProjectGenerationController {
  private sessionId: number | null = null
  private revision = 0
  private requestGeneration = 0

  currentSession(): number | null {
    return this.sessionId
  }

  adopt(snapshot: Pick<ProjectSnapshot, 'session_id' | 'revision'>): number | null {
    const previous = this.sessionId
    this.sessionId = snapshot.session_id
    this.revision = snapshot.revision
    this.requestGeneration += 1
    return previous
  }

  acceptSnapshot(snapshot: Pick<ProjectSnapshot, 'session_id' | 'revision'>): boolean {
    if (snapshot.session_id !== this.sessionId || snapshot.revision <= this.revision) return false
    this.revision = snapshot.revision
    this.requestGeneration += 1
    return true
  }

  acceptMutation(sessionId: number, revision: number): boolean {
    if (sessionId !== this.sessionId || revision < this.revision) return false
    if (revision > this.revision) this.requestGeneration += 1
    this.revision = revision
    return true
  }

  isCurrent(sessionId: number, revision: number): boolean {
    return this.sessionId === sessionId && this.revision === revision
  }

  beginRequest(): number {
    this.requestGeneration += 1
    return this.requestGeneration
  }

  captureRequest(): number {
    return this.requestGeneration
  }

  isRequestCurrent(request: number): boolean {
    return request === this.requestGeneration
  }
}

export type EditEntry = FieldEditEntry | InsertEditEntry | DeleteEditEntry

export interface FieldEditEntry {
  kind: 'field'
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  oldValue: FieldValue
  newValue: FieldValue
}

export interface InsertEditEntry {
  kind: 'insert'
  filePath: string
  coordinate: RecordCoordinate
  fields: FieldValue
}

export interface DeleteEditEntry {
  kind: 'delete'
  filePath: string
  coordinate: RecordCoordinate
  snapshot: FieldValue
}

export interface MutationHistorySnapshot {
  undo: readonly EditEntry[]
  redo: readonly EditEntry[]
  busy: boolean
}

type HistoryDirection = 'undo' | 'redo'
type HistoryExecutor = (entry: EditEntry) => Promise<MutationResult<unknown>>

const EMPTY_HISTORY: MutationHistorySnapshot = { undo: [], redo: [], busy: false }

export class MutationHistoryController {
  private snapshot: MutationHistorySnapshot = EMPTY_HISTORY
  private readonly listeners = new Set<() => void>()

  getSnapshot = (): MutationHistorySnapshot => this.snapshot

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  clear(): void {
    this.publish(EMPTY_HISTORY)
  }

  record(entry: EditEntry): void {
    this.publish({ undo: [...this.snapshot.undo, entry], redo: [], busy: this.snapshot.busy })
  }

  rebind(oldCoordinate: RecordCoordinate, newCoordinate: RecordCoordinate): void {
    if (sameCoordinate(oldCoordinate, newCoordinate)) return
    this.publish({
      ...this.snapshot,
      undo: this.snapshot.undo.map(entry => rebindEntry(entry, oldCoordinate, newCoordinate)),
      redo: this.snapshot.redo.map(entry => rebindEntry(entry, oldCoordinate, newCoordinate)),
    })
  }

  undo(execute: HistoryExecutor): Promise<MutationResult<unknown>> {
    return this.run('undo', execute)
  }

  redo(execute: HistoryExecutor): Promise<MutationResult<unknown>> {
    return this.run('redo', execute)
  }

  private async run(direction: HistoryDirection, execute: HistoryExecutor): Promise<MutationResult<unknown>> {
    if (this.snapshot.busy) return superseded()
    const source = direction === 'undo' ? this.snapshot.undo : this.snapshot.redo
    const entry = source[source.length - 1]
    if (!entry) return superseded()

    this.publish({ ...this.snapshot, busy: true })
    let result: MutationResult<unknown>
    try {
      result = await execute(entry)
    } catch {
      result = failed()
    }

    const currentSource = direction === 'undo' ? this.snapshot.undo : this.snapshot.redo
    const topIsUnchanged = currentSource[currentSource.length - 1] === entry
    if (result.status === 'committed' && topIsUnchanged) {
      this.publish(direction === 'undo'
        ? {
            undo: this.snapshot.undo.slice(0, -1),
            redo: [...this.snapshot.redo, entry],
            busy: false,
          }
        : {
            undo: [...this.snapshot.undo, entry],
            redo: this.snapshot.redo.slice(0, -1),
            busy: false,
          })
    } else {
      this.publish({ ...this.snapshot, busy: false })
    }
    return result
  }

  private publish(snapshot: MutationHistorySnapshot): void {
    if (snapshot === this.snapshot) return
    this.snapshot = snapshot
    for (const listener of this.listeners) listener()
  }
}

function rebindEntry(
  entry: EditEntry,
  oldCoordinate: RecordCoordinate,
  newCoordinate: RecordCoordinate,
): EditEntry {
  if (!sameCoordinate(entry.coordinate, oldCoordinate)) return entry
  return { ...entry, coordinate: newCoordinate }
}
