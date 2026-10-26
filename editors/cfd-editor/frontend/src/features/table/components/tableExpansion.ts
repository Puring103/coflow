import type { FieldAnnotation } from '../../../bindings/FieldAnnotation'
import { annotationChild, annotationItem, dictKeyPathText, objectFieldCells, presentationValue, type FieldValue } from '../../../wire'
import { createContext, useContext, useSyncExternalStore } from 'react'

const emptyPaths: ReadonlySet<string> = new Set()
/** 展开状态属于文件视图；窗口卸载只释放 DOM，不丢失用户的展开路径。 */
export class TableExpansionStore {
  private paths = new Map<string, ReadonlySet<string>>()
  private listeners = new Map<string, Set<() => void>>()
  get = (key: string) => this.paths.get(key) ?? emptyPaths
  subscribe = (key: string, listener: () => void) => {
    let listeners = this.listeners.get(key)
    if (!listeners) this.listeners.set(key, listeners = new Set())
    listeners.add(listener)
    return () => { listeners.delete(listener); if (!listeners.size) this.listeners.delete(key) }
  }
  toggle(key: string, path: string, expanded: boolean) {
    const current = this.get(key)
    if (current.has(path) === expanded) return
    const next = new Set(current)
    if (expanded) next.add(path)
    else next.delete(path)
    if (next.size) this.paths.set(key, next)
    else this.paths.delete(key)
    this.listeners.get(key)?.forEach(listener => listener())
  }
}
export const TableExpansionContext = createContext<TableExpansionStore | null>(null)
export function useTableExpansion(key: string) {
  const store = useContext(TableExpansionContext)
  const paths = useSyncExternalStore(listener => store?.subscribe(key, listener) ?? (() => {}), () => store?.get(key) ?? emptyPaths)
  return { paths: store ? paths : undefined, toggle: store ? (path: string, expanded: boolean) => store.toggle(key, path, expanded) : undefined }
}

/** 只遍历高度预算内的展开行；未访问的尾部不构造路径、不读取子树。 */
export function previewWindow(value: FieldValue, annotation: FieldAnnotation | null | undefined, paths: ReadonlySet<string>, prefix: string, budget: number) {
  const windows = new Map<string, number>()
  let remaining = Math.max(1, budget)
  function visit(original: FieldValue, ann: FieldAnnotation | null | undefined, path: string) {
    const shown = presentationValue(original)
    const fields = shown.kind === 'object' ? objectFieldCells(shown, ann) : null
    const total = fields ? fields.length : shown.kind === 'array' || shown.kind === 'dict' ? shown.value.length : 0
    if (!total && (shown.kind === 'array' || shown.kind === 'dict') && remaining > 0) remaining--
    let count = 0
    while (count < total && remaining > 0) {
      const index = count++
      remaining--
      let child: FieldValue, childAnn: FieldAnnotation | null | undefined, childPath: string
      if (fields) {
        const field = fields[index]
        child = field.value; childAnn = field.annotation; childPath = path ? `${path}.${field.name}` : field.name
      } else if (shown.kind === 'array') {
        child = shown.value[index]; childAnn = annotationChild(ann, index) ?? annotationItem(ann); childPath = `${path}[${index}]`
      } else if (shown.kind === 'dict') {
        const [key, item] = shown.value[index], text = dictKeyPathText(key)
        child = item; childAnn = annotationChild(ann, text) ?? annotationItem(ann); childPath = `${path}[${text}]`
      } else break
      if (remaining > 0 && paths.has(childPath)) visit(child, childAnn, childPath)
    }
    windows.set(path, count)
  }
  visit(value, annotation, prefix)
  return { windows, rows: Math.max(1, Math.max(1, budget) - remaining) }
}

/** 仅适配行高时完整统计展开树；折叠节点不访问子树。 */
export function previewRowCount(value: FieldValue, annotation: FieldAnnotation | null | undefined, paths: ReadonlySet<string>, prefix: string): number {
  const shown = presentationValue(value)
  const descendants = (child: FieldValue, childAnnotation: FieldAnnotation | null | undefined, path: string) => (
    paths.has(path) ? previewRowCount(child, childAnnotation, paths, path) : 0
  )
  if (shown.kind === 'object') {
    const fields = objectFieldCells(shown, annotation)
    return Math.max(1, fields.length + (paths.size ? fields.reduce((sum, field) => sum + descendants(field.value, field.annotation, `${prefix}.${field.name}`), 0) : 0))
  }
  if (shown.kind === 'array') return Math.max(1, shown.value.length + (paths.size ? shown.value.reduce((sum, child, index) => {
    return sum + descendants(child, annotationChild(annotation, index) ?? annotationItem(annotation), `${prefix}[${index}]`)
  }, 0) : 0))
  if (shown.kind === 'dict') return Math.max(1, shown.value.length + (paths.size ? shown.value.reduce((sum, [key, child]) => {
    const text = dictKeyPathText(key)
    return sum + descendants(child, annotationChild(annotation, text) ?? annotationItem(annotation), `${prefix}[${text}]`)
  }, 0) : 0))
  return 0
}
