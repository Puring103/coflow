import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { DeleteRecordOutcome } from '../bindings/DeleteRecordOutcome'
import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { BatchWriteFieldOutcome } from '../bindings/BatchWriteFieldOutcome'
import type { DimensionValueCoordinate } from '../bindings/DimensionValueCoordinate'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import type { FileRecords } from '../bindings/FileRecords'
import type { InsertRecordOutcome } from '../bindings/InsertRecordOutcome'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { RenameRecordOutcome } from '../bindings/RenameRecordOutcome'
import type { WriteFieldOutcome } from '../bindings/WriteFieldOutcome'
import type { WriteDimensionValueOutcome } from '../bindings/WriteDimensionValueOutcome'
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
  writeFields: (
    sessionId: number,
    writes: BatchWriteFieldInput[],
  ) => Promise<BatchWriteFieldOutcome>
  writeField: (
    sessionId: number,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => Promise<WriteFieldOutcome>
  writeDimensionValue: (
    sessionId: number,
    coordinate: DimensionValueCoordinate,
    expectedValue: DimensionValueState,
    newValue: DimensionValueState,
  ) => Promise<WriteDimensionValueOutcome>
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
  fileRecordsForRow?: (
    filePath: string,
    previousCoordinate: RecordCoordinate,
    row: RecordRow,
    revision: number,
  ) => FileRecords | undefined
  rebindCoordinate: (
    filePath: string,
    oldCoordinate: RecordCoordinate,
    newCoordinate: RecordCoordinate,
  ) => void
  removeCoordinate?: (filePath: string, coordinate: RecordCoordinate) => void
  recoverPublication: (request: MutationPublicationRequest, error: unknown) => boolean
  reportError: (
    sessionId: number,
    prefix: string,
    error: unknown,
    expectedRevision: number,
  ) => void
  optimisticWriteField?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => OptimisticFieldWrite
  optimisticWriteDimension?: (
    coordinate: DimensionValueCoordinate,
    newValue: DimensionValueState,
  ) => OptimisticDimensionWrite
}

export interface OptimisticFieldWrite {
  changed: boolean
  row?: RecordRow
  reapply: () => void
  rollback: () => void
}

export interface OptimisticDimensionWrite {
  changed: boolean
  reapply: () => void
  rollback: () => void
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
  private readonly pendingFieldWrites = new Map<string, PendingFieldWrite>()
  private readonly pendingDimensionWrites = new Map<string, PendingDimensionWrite>()

  constructor(
    private readonly backend: EditorMutationBackend,
    private readonly port: EditorMutationPort,
    private readonly history: MutationHistoryController,
  ) {}

  writeField(
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ): Promise<RecordRow | undefined> {
    const optimistic = this.port.optimisticWriteField?.(
      filePath,
      coordinate,
      fieldPath,
      newValue,
    )
    if (optimistic && !optimistic.changed) return Promise.resolve(optimistic.row)
    const key = fieldWriteKey(filePath, coordinate, fieldPath)
    const queuedGeneration = this.port.currentGeneration()
    const queuedHistoryEpoch = this.history.currentEpoch()
    const existing = this.pendingFieldWrites.get(key)
    if (
      existing
      && sameGeneration(existing.queuedGeneration, queuedGeneration)
      && existing.queuedHistoryEpoch === queuedHistoryEpoch
    ) {
      existing.newValue = cloneValue(newValue)
      if (optimistic) existing.optimistic.push(optimistic)
      return new Promise(resolve => existing.resolve.push(resolve))
    }
    const pending: PendingFieldWrite = {
      key,
      filePath,
      coordinate,
      fieldPath,
      newValue: cloneValue(newValue),
      optimistic: optimistic ? [optimistic] : [],
      resolve: [],
      queuedGeneration,
      queuedHistoryEpoch,
    }
    this.pendingFieldWrites.set(key, pending)
    const result = new Promise<RecordRow | undefined>(resolve => pending.resolve.push(resolve))
    queueMicrotask(() => this.flushFieldWrite(pending))
    return result
  }

  writeFields(
    filePath: string,
    coordinates: readonly RecordCoordinate[],
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ): Promise<void> {
    const writes = coordinates.map(coordinate => ({
      coordinate,
      field_path: fieldPath,
      new_value: cloneValue(newValue),
    }))
    return this.enqueueMutation(undefined, async () => {
      await this.writeFieldsInternal(filePath, writes, { recordHistory: true })
    })
  }

  writeDimensionValue(
    filePath: string,
    coordinate: DimensionValueCoordinate,
    expectedValue: DimensionValueState,
    newValue: DimensionValueState,
  ): Promise<DimensionValueState | undefined> {
    const optimistic = this.port.optimisticWriteDimension?.(coordinate, newValue)
    if (optimistic && !optimistic.changed) return Promise.resolve(newValue)
    const key = dimensionWriteKey(coordinate)
    const queuedGeneration = this.port.currentGeneration()
    const queuedHistoryEpoch = this.history.currentEpoch()
    const existing = this.pendingDimensionWrites.get(key)
    if (
      existing
      && sameGeneration(existing.queuedGeneration, queuedGeneration)
      && existing.queuedHistoryEpoch === queuedHistoryEpoch
    ) {
      existing.newValue = cloneDimensionState(newValue)
      if (optimistic) existing.optimistic.push(optimistic)
      return new Promise(resolve => existing.resolve.push(resolve))
    }
    const pending: PendingDimensionWrite = {
      key,
      filePath,
      coordinate,
      expectedValue: cloneDimensionState(expectedValue),
      newValue: cloneDimensionState(newValue),
      optimistic: optimistic ? [optimistic] : [],
      resolve: [],
      queuedGeneration,
      queuedHistoryEpoch,
    }
    this.pendingDimensionWrites.set(key, pending)
    const result = new Promise<DimensionValueState | undefined>(resolve => pending.resolve.push(resolve))
    queueMicrotask(() => this.flushDimensionWrite(pending))
    return result
  }

  private async flushFieldWrite(pending: PendingFieldWrite): Promise<void> {
    const result = await this.enqueueMutationFor(
      pending.queuedGeneration,
      pending.queuedHistoryEpoch,
      superseded(),
      async () => {
      if (this.pendingFieldWrites.get(pending.key) === pending) {
        this.pendingFieldWrites.delete(pending.key)
      }
        const result = await this.writeFieldInternal(
          pending.filePath,
          pending.coordinate,
          pending.fieldPath,
          pending.newValue,
          { recordHistory: true },
        )
        return result
      },
    )
    if (result.status !== 'committed') {
      for (const projection of [...pending.optimistic].reverse()) projection.rollback()
    }
    const row = committedValue(result)
    for (const resolve of pending.resolve) resolve(row)
  }

  private async flushDimensionWrite(pending: PendingDimensionWrite): Promise<void> {
    const result = await this.enqueueMutationFor(
      pending.queuedGeneration,
      pending.queuedHistoryEpoch,
      superseded(),
      async () => {
        if (this.pendingDimensionWrites.get(pending.key) === pending) {
          this.pendingDimensionWrites.delete(pending.key)
        }
        return this.writeDimensionValueInternal(
          pending.filePath,
          pending.coordinate,
          pending.expectedValue,
          pending.newValue,
          { recordHistory: true },
        )
      },
    )
    if (result.status !== 'committed') {
      for (const projection of [...pending.optimistic].reverse()) projection.rollback()
    }
    const value = committedValue(result)
    for (const resolve of pending.resolve) resolve(value)
  }

  private reapplyPendingFieldWrites(): void {
    for (const pending of this.pendingFieldWrites.values()) {
      if (!this.queuedMutationIsCurrent(
        pending.queuedGeneration,
        pending.queuedHistoryEpoch,
      )) continue
      for (const projection of pending.optimistic) projection.reapply()
    }
    for (const pending of this.pendingDimensionWrites.values()) {
      if (!this.queuedMutationIsCurrent(
        pending.queuedGeneration,
        pending.queuedHistoryEpoch,
      )) continue
      for (const projection of pending.optimistic) projection.reapply()
    }
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
      outcome => this.fileRecordsForRow(filePath, coordinate, outcome.row, outcome.revision),
      outcome => {
        const finalCoordinate = outcome.renamed ?? coordinate
        this.applyRename(filePath, coordinate, outcome.renamed)
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
      outcome => this.fileRecordsForRow(filePath, coordinate, outcome.row, outcome.revision),
      outcome => {
        this.applyRename(filePath, coordinate, outcome.renamed)
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
    await this.history.undo(entry => this.executeWithPendingFieldReplay<MutationResult<unknown>>(
      () => {
        if (entry.kind === 'dimension') {
          return this.writeDimensionValueInternal(
            entry.filePath,
            entry.coordinate,
            entry.newValue,
            entry.oldValue,
            { recordHistory: false },
          )
        }
        if (entry.kind === 'field') {
          return this.writeFieldInternal(
            entry.filePath,
            entry.coordinate,
            entry.fieldPath,
            entry.oldValue,
            { recordHistory: false },
          )
        }
        if (entry.kind === 'batch-field') {
          return this.writeFieldsInternal(
            entry.edits[0]?.filePath ?? '',
            entry.edits.map(edit => ({
              coordinate: edit.coordinate,
              field_path: edit.fieldPath,
              new_value: cloneValue(edit.oldValue),
            })),
            { recordHistory: false },
          )
        }
        if (entry.kind === 'insert') {
          return this.deleteRecordInternal(
            entry.filePath,
            entry.coordinate,
            { recordHistory: false },
          )
        }
        return this.insertRecordInternal(
          entry.filePath,
          entry.coordinate.key,
          entry.coordinate.actual_type,
          entry.snapshot,
          { recordHistory: false },
        )
      },
    ))
  }

  async redo(): Promise<void> {
    await this.history.redo(entry => this.executeWithPendingFieldReplay<MutationResult<unknown>>(
      () => {
        if (entry.kind === 'dimension') {
          return this.writeDimensionValueInternal(
            entry.filePath,
            entry.coordinate,
            entry.oldValue,
            entry.newValue,
            { recordHistory: false },
          )
        }
        if (entry.kind === 'field') {
          return this.writeFieldInternal(
            entry.filePath,
            entry.coordinate,
            entry.fieldPath,
            entry.newValue,
            { recordHistory: false },
          )
        }
        if (entry.kind === 'batch-field') {
          return this.writeFieldsInternal(
            entry.edits[0]?.filePath ?? '',
            entry.edits.map(edit => ({
              coordinate: edit.coordinate,
              field_path: edit.fieldPath,
              new_value: cloneValue(edit.newValue),
            })),
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
        return this.deleteRecordInternal(
          entry.filePath,
          entry.coordinate,
          { recordHistory: false },
        )
      },
    ))
  }

  private enqueueMutation<T>(supersededValue: T, operation: () => Promise<T>): Promise<T> {
    const queuedGeneration = this.port.currentGeneration()
    const queuedHistoryEpoch = this.history.currentEpoch()
    return this.enqueueMutationFor(
      queuedGeneration,
      queuedHistoryEpoch,
      supersededValue,
      operation,
    )
  }

  private enqueueMutationFor<T>(
    queuedGeneration: EditorGenerationIdentity | null,
    queuedHistoryEpoch: number,
    supersededValue: T,
    operation: () => Promise<T>,
  ): Promise<T> {
    return this.history.serialize(() => {
      if (!this.queuedMutationIsCurrent(queuedGeneration, queuedHistoryEpoch)) {
        return Promise.resolve(supersededValue)
      }
      return this.executeWithPendingFieldReplay(operation)
    })
  }

  private queuedMutationIsCurrent(
    queuedGeneration: EditorGenerationIdentity | null,
    queuedHistoryEpoch: number,
  ): boolean {
    const currentGeneration = this.port.currentGeneration()
    return queuedGeneration !== null
      && currentGeneration !== null
      && currentGeneration.sessionId === queuedGeneration.sessionId
      && currentGeneration.revision >= queuedGeneration.revision
      && this.history.currentEpoch() === queuedHistoryEpoch
  }

  private async executeWithPendingFieldReplay<T>(operation: () => Promise<T>): Promise<T> {
    const result = await operation()
    this.reapplyPendingFieldWrites()
    return result
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
      outcome => this.fileRecordsForRow(filePath, coordinate, outcome.row, outcome.revision),
      outcome => {
        const finalCoordinate = outcome.renamed ?? coordinate
        this.applyRename(filePath, coordinate, outcome.renamed)
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
      fieldWriteChangesTopology,
    )
  }

  private writeFieldsInternal(
    filePath: string,
    writes: BatchWriteFieldInput[],
    options: MutationOptions,
  ): Promise<MutationResult<void>> {
    return this.execute(
      '批量写入失败',
      filePath,
      sessionId => this.backend.writeFields(sessionId, writes),
      undefined,
      outcome => {
        for (const edit of outcome.edits) {
          this.applyRename(filePath, edit.coordinate, sameCoordinateOrNull(
            edit.coordinate,
            edit.final_coordinate,
          ))
        }
        if (options.recordHistory) {
          const edits = outcome.edits.flatMap(edit => (
            edit.old_value && edit.new_value
              ? [{
                  filePath,
                  coordinate: edit.final_coordinate,
                  fieldPath: edit.field_path,
                  oldValue: cloneValue(edit.old_value),
                  newValue: cloneValue(edit.new_value),
                }]
              : []
          ))
          if (edits.length > 0) {
            this.history.record({
              kind: 'batch-field',
              revision: outcome.revision,
              edits,
            })
          }
        }
      },
      outcome => outcome.edits.some(edit => (
        containsReference(edit.old_value) || containsReference(edit.new_value)
      )),
    )
  }

  private writeDimensionValueInternal(
    filePath: string,
    coordinate: DimensionValueCoordinate,
    expectedValue: DimensionValueState,
    newValue: DimensionValueState,
    options: MutationOptions,
  ): Promise<MutationResult<DimensionValueState>> {
    return this.execute(
      '维度值写入失败',
      filePath,
      sessionId => this.backend.writeDimensionValue(
        sessionId,
        coordinate,
        expectedValue,
        newValue,
      ),
      undefined,
      outcome => {
        if (options.recordHistory) {
          this.history.record({
            kind: 'dimension',
            revision: outcome.revision,
            filePath,
            coordinate: outcome.coordinate,
            oldValue: cloneDimensionState(outcome.old_value),
            newValue: cloneDimensionState(outcome.new_value),
          })
        }
        return outcome.new_value
      },
      outcome => containsDimensionReference(outcome.old_value)
        || containsDimensionReference(outcome.new_value),
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
        this.port.removeCoordinate?.(filePath, coordinate)
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
    knownRecords: ((outcome: TOutcome) => FileRecords | undefined) | undefined,
    afterCommit: (outcome: TOutcome) => TValue,
    topologyChanged: (outcome: TOutcome) => boolean = () => true,
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
        topologyChanged: topologyChanged(outcome),
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
    filePath: string,
    oldCoordinate: RecordCoordinate,
    newCoordinate: RecordCoordinate | null,
  ): void {
    if (!newCoordinate) return
    this.history.rebind(oldCoordinate, newCoordinate)
    this.port.rebindCoordinate(filePath, oldCoordinate, newCoordinate)
  }

  private fileRecordsForRow(
    filePath: string,
    previousCoordinate: RecordCoordinate,
    row: RecordRow,
    revision: number,
  ): FileRecords | undefined {
    return this.port.fileRecordsForRow?.(filePath, previousCoordinate, row, revision)
  }
}

interface PendingFieldWrite {
  key: string
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  newValue: FieldValue
  optimistic: OptimisticFieldWrite[]
  resolve: Array<(row: RecordRow | undefined) => void>
  queuedGeneration: EditorGenerationIdentity | null
  queuedHistoryEpoch: number
}

interface PendingDimensionWrite {
  key: string
  filePath: string
  coordinate: DimensionValueCoordinate
  expectedValue: DimensionValueState
  newValue: DimensionValueState
  optimistic: OptimisticDimensionWrite[]
  resolve: Array<(value: DimensionValueState | undefined) => void>
  queuedGeneration: EditorGenerationIdentity | null
  queuedHistoryEpoch: number
}

function fieldWriteKey(
  filePath: string,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
): string {
  return `${filePath}\u001f${coordinate.actual_type}\u001f${coordinate.key}\u001f${fieldPath
    .map(segment => `${segment.kind}:${segment.value}`)
    .join('/')}`
}

function dimensionWriteKey(coordinate: DimensionValueCoordinate): string {
  return [
    coordinate.actual_type,
    coordinate.record_key,
    coordinate.field,
    coordinate.dimension,
    coordinate.variant,
    ...coordinate.path.map(segment => `${segment.kind}:${segment.value}`),
  ].join('\u001f')
}

function cloneDimensionState(state: DimensionValueState): DimensionValueState {
  return state.kind === 'missing'
    ? { kind: 'missing' }
    : { kind: 'value', value: cloneValue(state.value) }
}

function sameGeneration(
  left: EditorGenerationIdentity | null,
  right: EditorGenerationIdentity | null,
): boolean {
  return left?.sessionId === right?.sessionId && left?.revision === right?.revision
}

function committedValue<T>(result: MutationResult<T>): T | undefined {
  return result.status === 'committed' ? result.value : undefined
}

function sameCoordinateOrNull(
  previous: RecordCoordinate,
  next: RecordCoordinate,
): RecordCoordinate | null {
  return previous.actual_type === next.actual_type && previous.key === next.key ? null : next
}

function fieldWriteChangesTopology(outcome: WriteFieldOutcome): boolean {
  return outcome.renamed !== null
    || containsReference(outcome.old_value)
    || containsReference(outcome.new_value)
}

function containsReference(value: FieldValue | null): boolean {
  if (!value) return false
  if (value.kind === 'ref') return true
  if (value.kind === 'object') {
    return Object.values(value.value.fields).some(field => field && containsReference(field))
  }
  if (value.kind === 'array') return value.value.some(containsReference)
  if (value.kind === 'dict') return value.value.some(([, item]) => containsReference(item))
  return false
}

function containsDimensionReference(state: DimensionValueState): boolean {
  return state.kind === 'value' && containsReference(state.value)
}
