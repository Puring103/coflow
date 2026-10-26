/** IPC 编解码：BigInt 与 wire 值的归一化唯一位置。 */
export function toIpc(value: unknown): unknown {
  if (typeof value === 'bigint') return value.toString()
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

// 仅复制包含整数转换的分支；字符串、注解与普通元数据直接共享接收对象。
// 不修改接收值，确保同一响应被多个消费者使用时仍遵守不可变快照约定。
function normalizeWireValue(value: unknown, enumPayload = false): unknown {
  if (!value || typeof value !== 'object') return value
  if (Array.isArray(value)) {
    let result: unknown[] | undefined
    value.forEach((item, index) => {
      const next = normalizeWireValue(item)
      if (next !== item) {
        result ??= value.slice()
        result[index] = next
      }
    })
    return result ?? value
  }
  const object = value as Record<string, unknown>
  let result: Record<string, unknown> | undefined
  for (const key of Object.keys(object)) {
    const item = object[key]
    const integer = (key === 'enum_int_value' && item !== null && typeof item !== 'object')
      || (key === 'value' && (object.kind === 'int' || enumPayload))
    const next = integer ? toBigInt(item) : normalizeWireValue(item, object.kind === 'enum' && key === 'value')
    if (next !== item) {
      result ??= { ...object }
      result[key] = next
    }
  }
  return result ?? value
}

function toBigInt(value: unknown): bigint {
  if (typeof value === 'bigint') return value
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new Error(`expected safe integer, got ${value}`)
    return BigInt(value)
  }
  if (typeof value === 'string') return BigInt(value)
  throw new Error(`expected integer, got ${String(value)}`)
}
