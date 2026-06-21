import * as api from '../api'
import type { FieldValue } from '../bindings/index'

let activeSessionId: number | null = null
const enumCache = new Map<string, string[]>()
const refCache = new Map<string, string[]>()

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
  } catch {
    return null
  }
}

export async function loadEnumVariants(enumName: string): Promise<string[] | null> {
  if (activeSessionId === null) return null
  if (enumCache.has(enumName)) return enumCache.get(enumName)!
  try {
    const r = await api.getEnumVariants(activeSessionId, enumName)
    enumCache.set(enumName, r)
    return r
  } catch {
    return null
  }
}

export async function loadRefTargets(targetType: string): Promise<string[] | null> {
  if (activeSessionId === null) return null
  if (refCache.has(targetType)) return refCache.get(targetType)!
  try {
    const r = await api.getRefTargets(activeSessionId, targetType)
    refCache.set(targetType, r)
    return r
  } catch {
    return null
  }
}
