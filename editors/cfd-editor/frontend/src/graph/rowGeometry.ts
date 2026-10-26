import type { FieldCell } from '../bindings/FieldCell'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import {
  dictKeyPathText, fieldPathDictKey, fieldPathField, fieldPathIndex, objectFieldCells,
  type FieldPathSegment, type FieldValue,
} from '../wire'
import { fieldRowPresentation } from '../value/rowPresentation'

const valueKeys = new WeakMap<FieldValue, string>()
function storageKey(value: FieldValue): string {
  let key = valueKeys.get(value)
  if (key === undefined) {
    key = JSON.stringify(value, (_name, item) => typeof item === 'bigint' ? `${item}n`
      : typeof item === 'number' && Object.is(item, -0) ? { negativeZero: true } : item)
    valueKeys.set(value, key)
  }
  return key
}

interface CollectionIdentity { items: FieldValue[]; keys?: string[]; epoch: number }
function shiftsArrayIndexes(previous: CollectionIdentity, items: FieldValue[]): boolean {
  if (previous.items === items) return false
  if (items.length < previous.items.length) return true
  const before = previous.keys ??= previous.items.map(storageKey)
  const after = items.map(storageKey)
  if (after.length > before.length) return !before.every((key, index) => after[index] === key)
  const different = after.reduce((count, key, index) => count + Number(before[index] !== key), 0)
  // 等长快照也可能合并了删除和插入；多项变化不能证明原索引身份仍然有效。
  return different >= 2
}

export interface GeometryRow {
  key: string
  path: string
  /** 同类固定控件共享实测高度；文本与异步内容逐行测量。 */
  sample: string | null
  last: boolean
}
interface Sibling { index: number; first: number; end: number }
export interface RowSlice { first: number; end: number; height: number; visible: boolean }

/** 前缀高度使用树状数组：单行变化与坐标查找均为 O(log R)。 */
class HeightIndex {
  private tree: Float64Array
  private heights: Float64Array
  constructor(values: number[]) {
    this.heights = Float64Array.from(values)
    this.tree = new Float64Array(values.length + 1)
    values.forEach((value, index) => { this.tree[index + 1] += value; const parent = (index + 1) + ((index + 1) & -(index + 1)); if (parent < this.tree.length) this.tree[parent] += this.tree[index + 1] })
  }
  height(index: number): number { return this.heights[index] ?? 0 }
  set(index: number, height: number): boolean {
    const difference = height - this.heights[index]
    if (Math.abs(difference) < 0.05) return false
    this.heights[index] = height
    for (let cursor = index + 1; cursor < this.tree.length; cursor += cursor & -cursor) this.tree[cursor] += difference
    return true
  }
  prefix(end: number): number {
    let result = 0
    for (let cursor = end; cursor > 0; cursor -= cursor & -cursor) result += this.tree[cursor]
    return result
  }
  at(y: number): number {
    let index = 0, sum = 0
    for (let bit = 2 ** Math.floor(Math.log2(this.tree.length)); bit > 0; bit >>= 1) {
      const next = index + bit
      if (next < this.tree.length && sum + this.tree[next] <= y) { index = next; sum += this.tree[next] }
    }
    return Math.min(index, this.heights.length)
  }
}

export class GraphRowGeometry {
  readonly rows: GeometryRow[] = []
  readonly byKey = new Map<string, number>()
  readonly byPath = new Map<string, number>()
  private siblings = new Map<string, Sibling[]>()
  private samples = new Map<string, number[]>()
  private sampleHeights = new Map<string, number>()
  private measured = new Map<string, number>()
  private dynamicValues = new Map<string, FieldValue>()
  private pending = new Set<number>()
  private dynamicRows = new Set<number>()
  private collections = new Map<string, CollectionIdentity>()
  private resetPaths = new Set<string>()
  resetVersion = 0
  private heights: HeightIndex
  private listeners = new Set<() => void>()
  private version = 0
  headerHeight = 36
  borderTop = 2.5
  borderBottom = 1

  constructor(fields: FieldCell[], expanded: ReadonlySet<string>, appendPaths: ReadonlySet<string>, editable: boolean, previous?: GraphRowGeometry, extra?: { collapsed?: boolean; more?: boolean }) {
    this.headerHeight = previous?.headerHeight ?? this.headerHeight
    this.borderTop = previous?.borderTop ?? this.borderTop
    this.borderBottom = previous?.borderBottom ?? this.borderBottom
    this.sampleHeights = new Map(previous?.sampleHeights)
    const initial: number[] = []
    const add = (key: string, path: string, sample: string | null, last: boolean, estimate: number, value?: FieldValue) => {
      const index = this.rows.length
      this.rows.push({ key, path, sample, last })
      this.byKey.set(key, index)
      this.byPath.set(path, index)
      if (sample) { const list = this.samples.get(sample) ?? []; list.push(index); this.samples.set(sample, list) }
      const oldValue = previous?.dynamicValues.get(key)
      const reusable = sample !== null || value === oldValue || (value && oldValue && value.kind === 'string' && oldValue.kind === 'string' && value.value === oldValue.value)
      if (!sample && value) this.dynamicValues.set(key, value)
      const old = reusable ? previous?.measured.get(key) : undefined
      if (old !== undefined) this.measured.set(key, old)
      if (old === undefined && (!sample || (!this.sampleHeights.has(sample) && this.samples.get(sample)?.length === 1))) this.pending.add(index)
      initial.push(old ?? (sample ? this.sampleHeights.get(sample) : undefined) ?? estimate)
      return index
    }
    const children = (items: { value: FieldValue; annotation?: FieldAnnotation | null; missing?: boolean; name: string; path: FieldPathSegment[]; display: string; editable: boolean }[], parent: string, appended = false) => {
      const siblings: Sibling[] = []
      this.siblings.set(parent, siblings)
      items.forEach((item, index) => {
        const first = this.rows.length, key = JSON.stringify(item.path)
        const { displayValue: value } = fieldRowPresentation(item.value, item.annotation?.declared_type ?? undefined)
        const complex = value.kind === 'object' || value.kind === 'array' || value.kind === 'dict'
        if (value.kind === 'array') {
          const old = previous?.collections.get(key)
          const shifted = old !== undefined && shiftsArrayIndexes(old, value.value)
          this.collections.set(key, { items: value.value, epoch: (old?.epoch ?? 0) + Number(shifted) })
          if (shifted) this.resetPaths.add(key)
        }
        const last = complex ? !expanded.has(item.display) : index === items.length - 1 && !appended
        const sample = !item.editable && (value.kind === 'string' || value.kind === 'formatted_string') ? null : JSON.stringify([
          value.kind, item.value.kind, item.annotation?.declared_type, item.annotation?.ref_target_type,
          item.annotation?.nullable, item.annotation?.enum_is_flag, item.annotation?.polymorphic_types.length,
          item.editable, !!item.missing, last, appendPaths.has(key),
        ])
        add(key, item.display, sample, last, complex ? (last ? 24 : 25) : item.editable ? (last ? 30 : 31) : (last ? 23 : 24), item.value)
        if (complex && expanded.has(item.display)) {
          let nested: typeof items = []
          if (value.kind === 'object') nested = objectFieldCells(value, item.annotation).map(field => ({
            name: field.name, value: field.value, annotation: field.annotation, missing: field.missing,
            path: [...item.path, fieldPathField(field.name)], display: `${item.display}.${field.name}`, editable: item.editable && !field.annotation?.read_only,
          }))
          else if (value.kind === 'array') nested = value.value.map((child, i) => ({
            name: String(i), value: child, annotation: item.annotation?.children[String(i)] ?? item.annotation?.item_annotation,
            path: [...item.path, fieldPathIndex(i)], display: `${item.display}[${i}]`, editable: item.editable,
          }))
          else if (value.kind === 'dict') nested = value.value.map(([dictKey, child]) => {
            const name = dictKeyPathText(dictKey)
            return { name, value: child, annotation: item.annotation?.children[name] ?? item.annotation?.item_annotation,
              path: [...item.path, fieldPathDictKey(name)], display: `${item.display}[${name}]`, editable: item.editable }
          })
          children(nested, key, item.editable && value.kind === 'array' && appendPaths.has(key))
          if (!nested.length && (value.kind === 'array' || value.kind === 'dict')) add(`empty:${key}`, `${item.display}[empty]`, 'empty', true, 18)
          if (item.editable && value.kind === 'array' && appendPaths.has(key)) add(`append:${key}`, `${item.display}[+]`, 'append', true, 24)
        }
        siblings.push({ index, first, end: this.rows.length })
      })
    }
    if (extra?.collapsed) add('collapsed', '', 'collapsed', true, 40)
    else children(fields.map(field => ({ name: field.name, value: field.value, annotation: field.annotation, missing: field.missing,
      path: [fieldPathField(field.name)], display: field.name, editable: editable && !field.annotation?.read_only })), '[]')
    if (extra?.more) add('more', '', 'more', true, 28)
    this.heights = new HeightIndex(initial)
    for (const old of previous?.dynamicRows ?? []) {
      const key = previous!.rows[old].key, current = this.byKey.get(key)
      if (current !== undefined && !this.resetsPath(key)) this.dynamicRows.add(current)
    }
  }

  subscribe = (listener: () => void): (() => void) => { this.listeners.add(listener); return () => this.listeners.delete(listener) }
  snapshot = (): number => this.version
  get bodyHeight(): number { return this.heights.prefix(this.rows.length) }
  get pendingCount(): number { return this.pending.size }
  collectionEpoch(path: string): number { return this.collections.get(path)?.epoch ?? 0 }
  resetsPath(path: string): boolean {
    return [...this.resetPaths].some(parent => path.startsWith(parent.slice(0, -1) + ','))
  }
  resetCollection(path: string): void {
    const collection = this.collections.get(path)
    if (!collection) return
    // 索引变更明确结束原集合行的编辑身份，不能把未提交草稿复用到另一元素。
    collection.epoch++
    this.resetPaths = new Set([path])
    this.resetVersion++
    this.version++
    for (const listener of this.listeners) listener()
  }
  probes(limit: number): number[] {
    const result: number[] = []
    for (const index of this.pending) { if (!this.dynamicRows.has(index)) result.push(index); if (result.length >= limit) break }
    return result
  }
  invalidateMeasurements(): void {
    this.sampleHeights.clear()
    this.measured.clear()
    this.pending = new Set(this.rows.flatMap((row, index) => row.sample === null ? [index] : []))
    for (const indexes of this.samples.values()) this.pending.add(indexes.find(index => !this.dynamicRows.has(index)) ?? indexes[0])
    this.version++
    for (const listener of this.listeners) listener()
  }
  get height(): number { return this.borderTop + this.headerHeight + this.bodyHeight + this.borderBottom }
  top(index: number): number { return this.borderTop + this.headerHeight + this.heights.prefix(index) }
  center(path: string): number {
    const index = this.byPath.get(path)
    return index === undefined ? this.borderTop + this.headerHeight / 2 : this.top(index) + this.heights.height(index) / 2
  }
  range(top: number, bottom: number): [number, number] {
    const origin = this.borderTop + this.headerHeight
    if (bottom < top || bottom < origin) return [0, 0]
    if (top >= this.height - this.borderBottom) return [this.rows.length, this.rows.length]
    return [this.heights.at(top - origin), Math.min(this.rows.length, this.heights.at(bottom - origin) + 1)]
  }
  /** 仅遍历命中的兄弟与固定活动行；大集合窗口变化不扫描全部元素。 */
  slices(parent: string, first: number, end: number, pinned: ReadonlySet<number>): RowSlice[] {
    const list = this.siblings.get(parent) ?? []
    const lower = (row: number) => {
      let left = 0, right = list.length
      while (left < right) { const mid = (left + right) >>> 1; if (list[mid].end <= row) left = mid + 1; else right = mid }
      return left
    }
    const selected = new Set<number>()
    if (end > first) for (let i = lower(first); i < list.length && list[i].first < end; i++) selected.add(i)
    for (const row of pinned) { const i = lower(row); if (i < list.length && list[i].first <= row) selected.add(i) }
    const result: RowSlice[] = []
    let cursor = 0
    const push = (from: number, to: number, visible: boolean) => {
      if (to > from) result.push({ first: from, end: to, visible, height: this.heights.prefix(list[to - 1].end) - this.heights.prefix(list[from].first) })
    }
    for (const index of [...selected].sort((a, b) => a - b)) { push(cursor, index, false); push(index, index + 1, true); cursor = index + 1 }
    push(cursor, list.length, false)
    return result
  }
  rowHeight(key: string): number { const index = this.byKey.get(key); return index === undefined ? 0 : this.heights.height(index) }
  measure(rows: ReadonlyMap<string, number>, header?: { height: number; top: number; bottom: number }, dynamicKeys: ReadonlySet<string> = new Set()): void {
    let changed = false
    const pendingBefore = this.pending.size
    if (header && (Math.abs(this.headerHeight - header.height) > 0.05 || this.borderTop !== header.top || this.borderBottom !== header.bottom)) {
      this.headerHeight = header.height; this.borderTop = header.top; this.borderBottom = header.bottom; changed = true
    }
    // 同一批 DOM 先全部读取，再统一发布高度和端口几何。
    const samples = new Map<string, number>()
    for (const [key, height] of rows) {
      const index = this.byKey.get(key)
      if (index === undefined || height <= 0) continue
      this.pending.delete(index)
      if (dynamicKeys.has(key)) this.dynamicRows.add(index)
      else this.dynamicRows.delete(index)
      this.measured.set(key, height)
      changed = this.heights.set(index, height) || changed
      const sample = this.rows[index].sample
      if (sample && !dynamicKeys.has(key)) samples.set(sample, height)
      else if (sample && !this.sampleHeights.has(sample)) {
        const other = this.samples.get(sample)?.find(candidate => !this.dynamicRows.has(candidate))
        if (other !== undefined) this.pending.add(other)
      }
    }
    for (const [sample, height] of samples) {
      if (Math.abs((this.sampleHeights.get(sample) ?? Number.NaN) - height) < 0.05) continue
      this.sampleHeights.set(sample, height)
      for (const index of this.samples.get(sample) ?? []) {
        this.pending.delete(index)
        if (!this.dynamicRows.has(index)) changed = this.heights.set(index, height) || changed
      }
    }
    if (changed || pendingBefore !== this.pending.size) { this.version++; for (const listener of this.listeners) listener() }
  }
}
