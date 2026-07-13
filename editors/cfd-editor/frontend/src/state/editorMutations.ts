import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { DeleteRecordOutcome } from '../bindings/DeleteRecordOutcome'
import type { FileRecords } from '../bindings/FileRecords'
import type { InsertRecordOutcome } from '../bindings/InsertRecordOutcome'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { RenameRecordOutcome } from '../bindings/RenameRecordOutcome'
import type { WriteFieldOutcome } from '../bindings/WriteFieldOutcome'
import {
  cloneValue,
  deletedSnapshotValue,
  fieldPathField,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import {
  committed,
  failed,
  MutationHistoryController,
  superseded,
  type MutationPublicationRequest,
  type MutationResult,
  type EditorGenerationIdentity,
} from './editorState'

export interface EditorMutationBackend {
  writeField: (
    sessionId: number,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => Promise<WriteFieldOutcome>
  editCollection: (
    sessionId: number,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    edit: CollectionEdit,
  ) => Promise<WriteFieldOutcome>
  renameRecordKey: (
    sessionId: number,
    coordinate: RecordCoordinate,
    newKey: string,
  ) => Promise<RenameRecordOutcome>
  insertRecord: (
    sessionId: number,
    filePath: string,
    recordKey: string,
    actualType: string,
    fields: FieldValue,
  ) => Promise<InsertRecordOutcome>
  deleteRecord: (
    sessionId: number,
    coordinate: RecordCoordinate,
  ) => Promise<DeleteRecordOutcome>
}

export interface EditorMutationPort {
  currentGeneration: () => EditorGenerationIdentity | null
  publish: (request: MutationPublicationRequest) => Promise<MutationResult<void>>
  rebindCoordinate: (oldCoordinate: RecordCoordinate, newCoordinate: RecordCoordinate) => void
  recoverPublication: (request: MutationPublicationRequest, error: unknown) => boolean
  reportError: (
    sessionId: number,
    prefix: string,
    error: unknown,
    expectedRevision: number,
  ) => void
}

interface MutationOutcome {
  revision: number
  diagnostics: MutationPublicationRequest['diagnostics']
  affected_files: string[]
}

interface MutationOptions {
  recordHistory: boolean
}

export class EditorMutationController {
  constructor(
    private readonly backend: EditorMutationBackend,
    private readonly port: EditorMutationPort,
    private readonly history: MutationHistoryController,
  ) {}

  async writeField(
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ): Promise<RecordRow | undefined> {
    return this.enqueueMutation(undefined, async () => committedValue(await this.writeFieldInternal(
        filePath,
        coordinate,
        fieldPath,
        newValue,
        { recordHistory: true },
      )))
  }

  async editCollection(
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    edit: CollectionEdit,
  ): Promise<RecordRow | undefined> {
    return this.enqueueMutation(undefined, async () => committedValue(await this.execute(
      '集合编辑失败',
      filePath,
      sessionId => this.backend.editCollection(sessionId, coordinate, fieldPath, edit),
      undefined,
      outcome => {
        const finalCoordinate = outcome.renamed ?? coordinate
        this.applyRename(coordinate, outcome.renamed)
        if (outcome.old_value && outcome.new_value) {
          this.history.record({
            kind: 'field',
            revision: outcome.revision,
            filePath,
            coordinate: finalCoordinate,
            fieldPath,
            oldValue: cloneValue(outcome.old_value),
            newValue: cloneValue(outcome.new_value),
          })
        }
        return outcome.row
      },
    )))
  }

  async renameRecord(
    filePath: string,
    coordinate: RecordCoordinate,
    newKey: string,
  ): Promise<RecordRow | undefined> {
    return this.enqueueMutation(undefined, async () => committedValue(await this.execute(
      '重命名失败',
      filePath,
      sessionId => this.backend.renameRecordKey(sessionId, coordinate, newKey),
      undefined,
      outcome => {
        this.applyRename(coordinate, outcome.renamed)
        this.history.record({
          kind: 'field',
          revision: outcome.revision,
          filePath,
          coordinate: outcome.renamed,
          fieldPath: [fieldPathField('id')],
          oldValue: { kind: 'string', value: coordinate.key },
          newValue: { kind: 'string', value: newKey },
        })
        return outcome.row
      },
    )))
  }

  async insertRecord(
    filePath: string,
    recordKey: string,
    actualType: string,
    fields: FieldValue,
  ): Promise<void> {
    await this.enqueueMutation(undefined, () => this.insertRecordInternal(
      filePath,
      recordKey,
      actualType,
      fields,
      { recordHistory: true },
    ))
  }

  async deleteRecord(filePath: string, coordinate: RecordCoordinate): Promise<void> {
    await this.enqueueMutation(undefined, () => (
      this.deleteRecordInternal(filePath, coordinate, { recordHistory: true })
    ))
  }

  async undo(): Promise<void> {
    await this.history.undo(entry => {
      if (entry.kind === 'field') {
        return this.writeFieldInternal(
          entry.filePath,
          entry.coordinate,
          entry.fieldPath,
          entry.oldValue,
          { recordHistory: false },
        )
      }
      if (entry.kind === 'insert') {
        return this.deleteRecordInternal(entry.filePath, entry.coordinate, { recordHistory: false })
      }
      return this.insertRecordInternal(
        entry.filePath,
        entry.coordinate.key,
        entry.coordinate.actual_type,
        entry.snapshot,
        { recordHistory: false },
      )
    })
  }

  async redo(): Promise<void> {
    await this.history.redo(entry => {
      if (entry.kind === 'field') {
        return this.writeFieldInternal(
          entry.filePath,
          entry.coordinate,
          entry.fieldPath,
          entry.newValue,
          { recordHistory: false },
        )
      }
      if (entry.kind === 'insert') {
        return this.insertRecordInternal(
          entry.filePath,
          entry.coordinate.key,
          entry.coordinate.actual_type,
          entry.fields,
          { recordHistory: false },
        )
      }
      return this.deleteRecordInternal(entry.filePath, entry.coordinate, { recordHistory: false })
    })
  }

  private enqueueMutation<T>(supersededValue: T, operation: () => Promise<T>): Promise<T> {
    const queuedGeneration = this.port.currentGeneration()
    const queuedHistoryEpoch = this.history.currentEpoch()
    return this.history.serialize(() => {
      const currentGeneration = this.port.currentGeneration()
      if (
        !queuedGeneration
        || !currentGeneration
        || currentGeneration.sessionId !== queuedGeneration.sessionId
        || currentGeneration.revision < queuedGeneration.revision
        || this.history.currentEpoch() !== queuedHistoryEpoch
      ) {
        return Promise.resolve(supersededValue)
      }
      return operation()
    })
  }

  private writeFieldInternal(
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
    options: MutationOptions,
  ): Promise<MutationResult<RecordRow>> {
    return this.execute(
      '写入失败',
      filePath,
      sessionId => this.backend.writeField(sessionId, coordinate, fieldPath, newValue),
      undefined,
      outcome => {
        const finalCoordinate = outcome.renamed ?? coordinate
        this.applyRename(coordinate, outcome.renamed)
        if (options.recordHistory) {
          const oldValue = outcome.old_value
          const historyNewValue = outcome.new_value ?? newValue
          if (oldValue) {
            this.history.record({
              kind: 'field',
              revision: outcome.revision,
              filePath,
              coordinate: finalCoordinate,
              fieldPath,
              oldValue: cloneValue(oldValue),
              newValue: cloneValue(historyNewValue),
            })
          }
        }
        return outcome.row
      },
    )
  }

  private insertRecordInternal(
    filePath: string,
    recordKey: string,
    actualType: string,
    fields: FieldValue,
    options: MutationOptions,
  ): Promise<MutationResult<void>> {
    return this.execute(
      '新建记录失败',
      filePath,
      sessionId => this.backend.insertRecord(sessionId, filePath, recordKey, actualType, fields),
      outcome => outcome.file_records,
      outcome => {
        if (options.recordHistory) {
          this.history.record({
            kind: 'insert',
            revision: outcome.revision,
            filePath,
            coordinate: { actual_type: actualType, key: recordKey },
            fields: cloneValue(fields),
          })
        }
      },
    )
  }

  private deleteRecordInternal(
    filePath: string,
    coordinate: RecordCoordinate,
    options: MutationOptions,
  ): Promise<MutationResult<void>> {
    return this.execute(
      '删除记录失败',
      filePath,
      sessionId => this.backend.deleteRecord(sessionId, coordinate),
      outcome => outcome.file_records,
      outcome => {
        if (options.recordHistory && outcome.deleted_snapshot) {
          this.history.record({
            kind: 'delete',
            revision: outcome.revision,
            filePath,
            coordinate,
            snapshot: deletedSnapshotValue(outcome.deleted_snapshot),
          })
        }
      },
    )
  }

  private async execute<TOutcome extends MutationOutcome, TValue>(
    errorPrefix: string,
    fallbackFile: string,
    invoke: (sessionId: number) => Promise<TOutcome>,
    knownRecords: ((outcome: TOutcome) => FileRecords) | undefined,
    afterCommit: (outcome: TOutcome) => TValue,
  ): Promise<MutationResult<TValue>> {
    const generation = this.port.currentGeneration()
    if (!generation) return failed()
    const historyEpoch = this.history.currentEpoch()
    try {
      const outcome = await invoke(generation.sessionId)
      const request: MutationPublicationRequest = {
        sessionId: generation.sessionId,
        revision: outcome.revision,
        diagnostics: outcome.diagnostics,
        affectedFiles: outcome.affected_files,
        fallbackFile,
        knownRecords: knownRecords?.(outcome),
      }
      let publication: MutationResult<void>
      try {
        publication = await this.port.publish(request)
      } catch (error) {
        if (!this.port.recoverPublication(request, error)) return superseded()
        this.port.reportError(
          generation.sessionId,
          '编辑已保存，但刷新失败',
          error,
          outcome.revision,
        )
        return committed(afterCommit(outcome))
      }
      if (publication.status === 'superseded') {
        if (this.history.currentEpoch() === historyEpoch) {
          return committed(afterCommit(outcome))
        }
        return superseded()
      }
      if (publication.status === 'failed') return failed()
      return committed(afterCommit(outcome))
    } catch (error) {
      this.port.reportError(
        generation.sessionId,
        errorPrefix,
        error,
        generation.revision,
      )
      return failed()
    }
  }

  private applyRename(
    oldCoordinate: RecordCoordinate,
    newCoordinate: RecordCoordinate | null,
  ): void {
    if (!newCoordinate) return
    this.history.rebind(oldCoordinate, newCoordinate)
    this.port.rebindCoordinate(oldCoordinate, newCoordinate)
  }
}

function committedValue<T>(result: MutationResult<T>): T | undefined {
  return result.status === 'committed' ? result.value : undefined
}
