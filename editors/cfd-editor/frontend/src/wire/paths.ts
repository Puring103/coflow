import type { CfdPathSegment as FieldPathSegment } from '../bindings/CfdPathSegment'

/** 顶层字段名：同一正则唯一实现，四处调用统一引用此处。 */
export function topLevelFieldName(path: string): string {
  const match = path.match(/^[^.[]+/)
  return match ? match[0] : path
}

/** 字段路径段数组的顶层段（与字符串版语义一致）。 */
export function topLevelSegmentOfFieldPath(segments: Array<{ kind: string; value?: unknown } | string>): string {
  const first = segments[0]
  if (typeof first === 'string') return topLevelFieldName(first)
  if (first && typeof first === 'object' && 'kind' in first) {
    if (first.kind === 'field' && typeof (first as { value?: unknown }).value === 'string') {
      return (first as { value: string }).value
    }
  }
  return ''
}

export function fieldPathField(name: string): FieldPathSegment {
  return { kind: 'field', value: name }
}

export function fieldPathIndex(index: number): FieldPathSegment {
  return { kind: 'index', value: index }
}

export function fieldPathDictKey(value: string): FieldPathSegment {
  return { kind: 'dict_key', value }
}

/** Enumerate every strict-prefix path of a nested field path. Used to decide
 *  which foldouts must auto-expand when we highlight a diagnostic anchor
 *  buried inside an object/array. */
export function ancestorFieldPaths(fieldPath: string): string[] {
  const out: string[] = []
  let cur = fieldPath
  while (true) {
    const lastDot = cur.lastIndexOf('.')
    const lastBracket = cur.lastIndexOf('[')
    const cut = Math.max(lastDot, lastBracket)
    if (cut <= 0) break
    cur = cur.slice(0, cut)
    out.push(cur)
  }
  return out
}
