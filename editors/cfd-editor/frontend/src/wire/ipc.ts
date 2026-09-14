/** IPC 编解码：BigInt 与 wire 值的归一化唯一位置。 */
export function toIpc(value: unknown): unknown {
  if (typeof value === 'bigint') {
    return value.toString()
  }
  if (Array.isArray(value)) return value.map(toIpc)
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(value)) out[key] = toIpc(item)
    return out
  }
  return value
}

export function fromIpc<T>(value: T): T {
  return normalizeWireValue(value) as T
}

function normalizeWireValue(value: unknown): unknown {
  if (!value || typeof value !== 'object') return value
  if (Array.isArray(value)) return value.map(normalizeWireValue)

  const object = value as Record<string, unknown>
  if (typeof object.kind === 'string') {
    return normalizeTaggedWireObject(object)
  }

  const out: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(object)) {
    out[key] = key === 'enum_int_value' && item !== null
      ? toBigInt(item)
      : normalizeWireValue(item)
  }
  return out
}

function normalizeTaggedWireObject(object: Record<string, unknown>): unknown {
  const kind = object.kind
  switch (kind) {
    case 'int':
      return { ...object, value: toBigInt(object.value) }
    case 'enum':
      return { ...object, value: normalizeEnumWireValue(object.value) }
    case 'object':
      return { ...object, value: normalizeWireValue(object.value) }
    case 'option_some':
    case 'result_ok':
    case 'result_err':
      return { ...object, value: normalizeWireValue(object.value) }
    case 'array':
      return {
        ...object,
        value: Array.isArray(object.value)
          ? object.value.map(normalizeWireValue)
          : object.value,
      }
    case 'dict':
      return {
        ...object,
        value: Array.isArray(object.value)
          ? object.value.map((entry) => Array.isArray(entry)
            ? [normalizeWireValue(entry[0]), normalizeWireValue(entry[1])]
            : normalizeWireValue(entry))
          : object.value,
      }
    default:
      return normalizePlainObject(object)
  }
}

function normalizePlainObject(object: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(object)) out[key] = normalizeWireValue(item)
  return out
}

function normalizeEnumWireValue(value: unknown): unknown {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return value
  const enumObject = value as Record<string, unknown>
  return {
    ...enumObject,
    value: toBigInt(enumObject.value),
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

