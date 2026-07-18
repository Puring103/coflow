import type { ProjectSnapshot } from '../bindings/ProjectSnapshot'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { FileRecords } from '../bindings/FileRecords'
import type { DimensionValueCoordinate } from '../bindings/DimensionValueCoordinate'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import { sameCoordinate, type FieldPathSegment, type FieldValue } from '../wire'

export type MutationResult<T = void> =
  | { status: 'committed'; value: T }
  | { status: 'superseded' }
  | { status: 'failed' }

export const committed = <T>(value: T): MutationResult<T> => ({ status: 'committed', value })
export const superseded = (): MutationResult<never> => ({ status: 'superseded' })
export const failed = (): MutationResult<never> => ({ status: 'failed' })

export interface EditorGenerationIdentity {
  sessionId: number
  revision: number
}

export class ProjectGenerationController {
  private sessionId: number | null = null
  private revision = 0
  private requestGeneration = 0
  private projectRequestGeneration = 0

  currentSession(): number | null {
    return this.sessionId
  }

  currentIdentity(): EditorGenerationIdentity | null {
    return this.sessionId === null
      ? null
      : { sessionId: this.sessionId, revision: this.revision }
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

  beginProjectRequest(): number {
    this.projectRequestGeneration += 1
    return this.projectRequestGeneration
  }

  isProjectRequestCurrent(request: number): boolean {
    return request === this.projectRequestGeneration
  }

  captureRequest(): number {
    return this.requestGeneration
  }

  isRequestCurrent(request: number): boolean {
    return request === this.requestGeneration
  }
}

export interface MutationPublicationRequest {
  sessionId: number
  revision: number
  diagnostics: ProjectSnapshot['diagnostics']
  affectedFiles: readonly string[]
  fallbackFile: string
  knownRecords?: FileRecords
  topologyChanged?: boolean
}

export interface MutationPublicationPort {
  acceptRevision: (
    sessionId: number,
    revision: number,
    diagnostics: ProjectSnapshot['diagnostics'],
  ) => boolean
  isCurrent: (sessionId: number, revision: number) => boolean
  getFileRecords: (sessionId: number, filePath: string) => Promise<FileRecords>
  publishFileRecords: (records: readonly (readonly [string, FileRecords])[]) => void
  publishGraphProjection?: (
    revision: number,
    records: readonly FileRecords[],
    topologyChanged: boolean,
  ) => void
}

/**
 * Publishes one backend mutation generation. The backend revision is the only
 * ordering authority: a later caller must never suppress a newer revision.
 */
export async function publishMutationGeneration(
  port: MutationPublicationPort,
  request: MutationPublicationRequest,
): Promise<MutationResult<void>> {
  const {
    sessionId,
    revision,
    diagnostics,
    affectedFiles,
    fallbackFile,
    knownRecords,
    topologyChanged = true,
  } = request
  const files = Array.from(new Set([...affectedFiles, fallbackFile]))
  const refreshedFiles = await Promise.all(files.map(async file => {
    const records = knownRecords?.file_path === file && knownRecords.revision === revision
      ? knownRecords
      : await port.getFileRecords(sessionId, file)
    return [file, records] as const
  }))
  if (
    refreshedFiles.some(([, records]) => records.revision !== revision)
  ) {
    return superseded()
  }

  if (!port.acceptRevision(sessionId, revision, diagnostics)) return superseded()
  if (!port.isCurrent(sessionId, revision)) return superseded()
  port.publishGraphProjection?.(
    revision,
    refreshedFiles.map(([, records]) => records),
    topologyChanged,
  )
  port.publishFileRecords(refreshedFiles)
  return committed(undefined)
}

export type EditEntry = FieldEditEntry | BatchFieldEditEntry | DimensionEditEntry | InsertEditEntry | DeleteEditEntry

export interface FieldEditEntry {
  kind: 'field'
  revision: number
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  oldValue: FieldValue
  newValue: FieldValue
}

export interface BatchFieldEditEntry {
  kind: 'batch-field'
  revision: number
  edits: Array<Omit<FieldEditEntry, 'kind' | 'revision'>>
}

export interface DimensionEditEntry {
  kind: 'dimension'
  revision: number
  filePath: string
  coordinate: DimensionValueCoordinate
  oldValue: DimensionValueState
  newValue: DimensionValueState
}

export interface InsertEditEntry {
  kind: 'insert'
  revision: number
  filePath: string
  coordinate: RecordCoordinate
  fields: FieldValue
}

export interface DeleteEditEntry {
  kind: 'delete'
  revision: number
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
  private queue: Promise<void> = Promise.resolve()
  private epoch = 0

  getSnapshot = (): MutationHistorySnapshot => this.snapshot

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  clear(): void {
    this.epoch += 1
    this.publish(EMPTY_HISTORY)
  }

  currentEpoch(): number {
    return this.epoch
  }

  record(entry: EditEntry): void {
    const undo = [...this.snapshot.undo, entry]
    undo.sort((left, right) => left.revision - right.revision)
    this.publish({ undo, redo: [], busy: this.snapshot.busy })
  }

  rebind(oldCoordinate: RecordCoordinate, newCoordinate: RecordCoordinate): void {
    if (sameCoordinate(oldCoordinate, newCoordinate)) return
    this.publish({
      ...this.snapshot,
      undo: this.snapshot.undo.map(entry => rebindEntry(entry, oldCoordinate, newCoordinate)),
      redo: this.snapshot.redo.map(entry => rebindEntry(entry, oldCoordinate, newCoordinate)),
    })
  }

  serialize<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.queue.then(operation, operation)
    this.queue = result.then(() => undefined, () => undefined)
    return result
  }

  undo(execute: HistoryExecutor): Promise<MutationResult<unknown>> {
    return this.serialize(() => this.run('undo', execute))
  }

  redo(execute: HistoryExecutor): Promise<MutationResult<unknown>> {
    return this.serialize(() => this.run('redo', execute))
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
  if (entry.kind === 'batch-field') {
    return {
      ...entry,
      edits: entry.edits.map(edit => sameCoordinate(edit.coordinate, oldCoordinate)
        ? { ...edit, coordinate: newCoordinate }
        : edit),
    }
  }
  if (entry.kind === 'dimension') {
    if (
      entry.coordinate.actual_type !== oldCoordinate.actual_type
      || entry.coordinate.record_key !== oldCoordinate.key
    ) return entry
    return {
      ...entry,
      coordinate: {
        ...entry.coordinate,
        actual_type: newCoordinate.actual_type,
        record_key: newCoordinate.key,
      },
    }
  }
  if (!sameCoordinate(entry.coordinate, oldCoordinate)) return entry
  return { ...entry, coordinate: newCoordinate }
}
