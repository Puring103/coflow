import * as api from '../api'
import type { RefTarget } from '../bindings/RefTarget'
import type { FieldValue } from '../wire'

let activeSessionId: number | null = null
const enumCache = new Map<string, string[]>()
const refCache = new Map<string, RefTarget[]>()

export function setActiveSession(id: number | null) {
  if (activeSessionId !== id) {
    activeSessionId = id
    enumCache.clear()
    refCache.clear()
  }
}

export async function buildDefaultObject(typeName: string): Promise<FieldValue | null> {
  if (activeSessionId === null) return null
  try {
    return await api.makeDefaultObject(activeSessionId, typeName)
  } catch (err) {
    // Surface schema-materialization errors so the user knows why a
    // type switch didn't produce a value (abstract fields without a
    // default, singleton nesting, etc.).
    alert(`无法创建 ${typeName} 的默认值：${errorMessage(err)}`)
    return null
  }
}

export type LoadResult =
  | { ok: true; variants: string[] }
  | { ok: false; error: string }

export type RefLoadResult =
  | { ok: true; targets: RefTarget[] }
  | { ok: false; error: string }

export async function loadEnumVariants(enumName: string): Promise<LoadResult> {
  if (activeSessionId === null) return { ok: false, error: '未打开会话' }
  const cached = enumCache.get(enumName)
  if (cached) return { ok: true, variants: cached }
  try {
    const r = await api.getEnumVariants(activeSessionId, enumName)
    enumCache.set(enumName, r)
    return { ok: true, variants: r }
  } catch (err) {
    return { ok: false, error: errorMessage(err) }
  }
}

export async function loadRefTargets(targetType: string): Promise<RefLoadResult> {
  if (activeSessionId === null) return { ok: false, error: '未打开会话' }
  const cached = refCache.get(targetType)
  if (cached) return { ok: true, targets: cached }
  try {
    const r = await api.getRefTargets(activeSessionId, targetType)
    refCache.set(targetType, r)
    return { ok: true, targets: r }
  } catch (err) {
    return { ok: false, error: errorMessage(err) }
  }
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try {
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}
