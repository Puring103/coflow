import type { FlatDiagnostic } from '../../../bindings/FlatDiagnostic'
import type { CreateRecordDraft } from '../../../bindings/CreateRecordDraft'
import type { RefTarget } from '../../../bindings/RefTarget'
import type { FieldValue } from '../../../wire'
import type { EnumVariantOption } from '../../../bindings/EnumVariantOption'
import type { EditorGenerationIdentity } from './editorState'

export interface EditorLookupBackend {
  getEnumVariants: (sessionId: number, enumName: string) => Promise<EnumVariantOption[]>
  getRefTargets: (sessionId: number, targetType: string) => Promise<RefTarget[]>
  makeDefaultObject: (sessionId: number, typeName: string) => Promise<FieldValue>
  createRecordDraft: (sessionId: number, actualType: string) => Promise<CreateRecordDraft>
}

export type LookupResult<T> =
  | { ok: true; value: T }
  | { ok: false; reason: 'unavailable' | 'superseded' | 'failed'; error?: string }

export class EditorLookupController {
  private failures: FlatDiagnostic[] = []
  private listeners = new Set<() => void>()
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getFailures = () => this.failures
  private updateFailure(key: string, message?: string) {
    const next = this.failures.filter(item => item.id !== key)
    if (message) next.push({ id: key, severity: 'error', code: 'lookup_load_failed', stage: 'editor', message, target: { kind: 'none' }, contexts: [] })
    if (JSON.stringify(next) === JSON.stringify(this.failures)) return
    this.failures = next
    this.listeners.forEach(listener => listener())
  }

  private generation: EditorGenerationIdentity | null = null
  private epoch = 0
  private refEpoch = 0
  private readonly values = new Map<string, unknown>()
  private readonly requests = new Map<string, Promise<LookupResult<unknown>>>()
  private readonly staleRefs = new Set<string>()

  constructor(private readonly backend: EditorLookupBackend) {}

  adopt(generation: EditorGenerationIdentity | null): void {
    if (
      this.generation?.sessionId === generation?.sessionId
      && this.generation?.revision === generation?.revision
    ) return
    const sessionChanged = this.generation?.sessionId !== generation?.sessionId
    this.generation = generation

    if (sessionChanged) {
      this.failures = []
      this.listeners.forEach(listener => listener())
      this.epoch += 1
      this.refEpoch += 1
      this.values.clear()
      this.requests.clear()
      this.staleRefs.clear()
      return
    }

    // Enum metadata and default objects are schema-scoped, so a data mutation
    // does not invalidate them. Reference targets are data-scoped: retain the
    // last successful value for synchronous display, but refresh it on access.
    this.invalidateRefTargets()
  }

  invalidateRefTargets(): void {
    this.refEpoch += 1
    for (const key of this.values.keys()) {
      if (key.startsWith('ref:')) this.staleRefs.add(key)
    }
    for (const key of this.requests.keys()) {
      if (key.startsWith('ref:')) this.requests.delete(key)
    }
  }

  cachedEnumVariants(enumName: string): EnumVariantOption[] | undefined {
    return this.values.get(`enum:${enumName}`) as EnumVariantOption[] | undefined
  }

  cachedRefTargets(targetType: string): RefTarget[] | undefined {
    return this.values.get(`ref:${targetType}`) as RefTarget[] | undefined
  }

  loadEnumVariants(enumName: string): Promise<LookupResult<EnumVariantOption[]>> {
    return this.lookup('enum', enumName, (sessionId) => (
      this.backend.getEnumVariants(sessionId, enumName)
    ))
  }

  loadRefTargets(targetType: string): Promise<LookupResult<RefTarget[]>> {
    const key = `ref:${targetType}`
    const refEpoch = this.refEpoch
    return this.lookup('ref', targetType, (sessionId) => (
      this.backend.getRefTargets(sessionId, targetType)
    ), {
      refresh: this.staleRefs.has(key),
      isCurrent: () => refEpoch === this.refEpoch,
      onSuccess: () => this.staleRefs.delete(key),
    })
  }

  makeDefaultObject(typeName: string): Promise<LookupResult<FieldValue>> {
    return this.lookup('default', typeName, (sessionId) => (
      this.backend.makeDefaultObject(sessionId, typeName)
    ))
  }

  createRecordDraft(actualType: string): Promise<LookupResult<CreateRecordDraft>> {
    return this.lookup('draft', actualType, (sessionId) => (
      this.backend.createRecordDraft(sessionId, actualType)
    ), { cache: false })
  }

  private lookup<T>(
    kind: string,
    name: string,
    load: (sessionId: number) => Promise<T>,
    options: {
      cache?: boolean
      refresh?: boolean
      isCurrent?: () => boolean
      onSuccess?: () => void
    } = {},
  ): Promise<LookupResult<T>> {
    const generation = this.generation
    if (!generation) return Promise.resolve({ ok: false, reason: 'unavailable' })

    const key = `${kind}:${name}`
    const cache = options.cache ?? true
    if (cache && !options.refresh && this.values.has(key)) {
      return Promise.resolve({ ok: true, value: this.values.get(key) as T })
    }
    const pending = this.requests.get(key)
    if (pending) return pending as Promise<LookupResult<T>>

    const epoch = this.epoch
    const isCurrent = () => epoch === this.epoch && (options.isCurrent?.() ?? true)
    const request = load(generation.sessionId)
      .then<LookupResult<T>>(value => {
        if (!isCurrent()) return { ok: false, reason: 'superseded' }
        if (cache) this.values.set(key, value)
        this.updateFailure(key)
        options.onSuccess?.()
        return { ok: true, value }
      })
      .catch((error: unknown): LookupResult<T> => {
        if (!isCurrent()) return { ok: false, reason: 'superseded' }
        const message = errorMessage(error)
        this.updateFailure(key, `${name}：${message}`)
        return { ok: false, reason: 'failed', error: message }
      })
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
