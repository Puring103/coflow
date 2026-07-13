import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RefTarget } from '../bindings/RefTarget'
import type { FieldValue } from '../wire'
import type { EditorGenerationIdentity } from './editorState'

export interface EditorLookupBackend {
  getEnumVariants: (sessionId: number, enumName: string) => Promise<string[]>
  getRefTargets: (sessionId: number, targetType: string) => Promise<RefTarget[]>
  makeDefaultObject: (sessionId: number, typeName: string) => Promise<FieldValue>
  createRecordDraft: (sessionId: number, actualType: string) => Promise<CreateRecordDraft>
}

export type LookupResult<T> =
  | { ok: true; value: T }
  | { ok: false; reason: 'unavailable' | 'superseded' | 'failed'; error?: string }

export class EditorLookupController {
  private generation: EditorGenerationIdentity | null = null
  private epoch = 0
  private readonly values = new Map<string, unknown>()
  private readonly requests = new Map<string, Promise<LookupResult<unknown>>>()

  constructor(private readonly backend: EditorLookupBackend) {}

  adopt(generation: EditorGenerationIdentity | null): void {
    if (
      this.generation?.sessionId === generation?.sessionId
      && this.generation?.revision === generation?.revision
    ) return
    this.generation = generation
    this.epoch += 1
    this.values.clear()
    this.requests.clear()
  }

  loadEnumVariants(enumName: string): Promise<LookupResult<string[]>> {
    return this.lookup('enum', enumName, (sessionId) => (
      this.backend.getEnumVariants(sessionId, enumName)
    ))
  }

  loadRefTargets(targetType: string): Promise<LookupResult<RefTarget[]>> {
    return this.lookup('ref', targetType, (sessionId) => (
      this.backend.getRefTargets(sessionId, targetType)
    ))
  }

  makeDefaultObject(typeName: string): Promise<LookupResult<FieldValue>> {
    return this.lookup('default', typeName, (sessionId) => (
      this.backend.makeDefaultObject(sessionId, typeName)
    ))
  }

  createRecordDraft(actualType: string): Promise<LookupResult<CreateRecordDraft>> {
    return this.lookup('draft', actualType, (sessionId) => (
      this.backend.createRecordDraft(sessionId, actualType)
    ), false)
  }

  private lookup<T>(
    kind: string,
    name: string,
    load: (sessionId: number) => Promise<T>,
    cache = true,
  ): Promise<LookupResult<T>> {
    const generation = this.generation
    if (!generation) return Promise.resolve({ ok: false, reason: 'unavailable' })

    const key = `${kind}:${name}`
    if (cache && this.values.has(key)) {
      return Promise.resolve({ ok: true, value: this.values.get(key) as T })
    }
    const pending = this.requests.get(key)
    if (pending) return pending as Promise<LookupResult<T>>

    const epoch = this.epoch
    const request = load(generation.sessionId)
      .then<LookupResult<T>>(value => {
        if (epoch !== this.epoch) return { ok: false, reason: 'superseded' }
        if (cache) this.values.set(key, value)
        return { ok: true, value }
      })
      .catch((error: unknown): LookupResult<T> => (
        epoch !== this.epoch
          ? { ok: false, reason: 'superseded' }
          : { ok: false, reason: 'failed', error: errorMessage(error) }
      ))
      .finally(() => {
        if (this.requests.get(key) === request) this.requests.delete(key)
      })
    this.requests.set(key, request as Promise<LookupResult<unknown>>)
    return request
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  try {
    return JSON.stringify(error)
  } catch {
    return String(error)
  }
}
