/** IPC 入参只复制需要转换 BigInt 的分支，避免整棵对象树重建。 */
export function toIpc(value: unknown): unknown {
  if (typeof value === 'bigint') return value.toString()
  if (!value || typeof value !== 'object') return value
  if (Array.isArray(value)) {
    const items = value.map(toIpc)
    return items.every((item, index) => item === value[index]) ? value : items
  }
  let result = value as Record<string, unknown>
  for (const [key, item] of Object.entries(value)) {
    const encoded = toIpc(item)
    if (encoded !== item) {
      if (result === value) result = { ...result }
      result[key] = encoded
    }
  }
  return result
}

/** 接管 invoke 返回的新对象，就地解码；此入口不得传入已经发布到 UI 的缓存对象。 */
export function fromIpc<T>(value: T): T {
  normalizeWireValue(value)
  return value
}

function normalizeWireValue(value: unknown): void {
  if (!value || typeof value !== 'object') return
  if (Array.isArray(value)) {
    for (const item of value) normalizeWireValue(item)
    return
  }
  const object = value as Record<string, unknown>
  if (object.kind === 'int') object.value = toBigInt(object.value)
  if (object.kind === 'enum' && object.value && typeof object.value === 'object') {
    const enumValue = object.value as Record<string, unknown>
    enumValue.value = toBigInt(enumValue.value)
  }
  for (const [key, item] of Object.entries(object)) {
    if (key === 'enum_int_value' && item !== null) object[key] = toBigInt(item)
    else normalizeWireValue(item)
  }
}

function toBigInt(value: unknown): bigint {
  if (typeof value === 'bigint') return value
  if (typeof value === 'number') {
    if (!Number.isInteger(value)) throw new Error(`expected integer, got ${value}`)
    return BigInt(value)
  }
  if (typeof value === 'string') return BigInt(value)
  throw new Error(`expected integer, got ${String(value)}`)
}

