// View model: resolves the implicit default views plus user-created custom
// views into a uniform shape the UI can render, and derives per-view
// projections (visible fields, group filter). Pure functions only — unit
// tested in views.test.ts.

import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { ViewConfig } from '../bindings/ViewConfig'

export const DEFAULT_RECORD_VIEW_ID = '__default_record'
export const DEFAULT_TABLE_VIEW_ID = '__default_table'
export const DEFAULT_SOURCE_VIEW_ID = '__default_source'
export const RESERVED_VIEW_ID_PREFIX = '__'

export type ViewRenderKind = 'record' | 'table' | 'graph' | 'source'

/** A tab in the view-tab row. */
export interface ViewTab {
  id: string
  name: string
  kind: ViewRenderKind
  isDefault: boolean
}

/** A view resolved to everything the renderer needs. Built-in views use `isDefault: true`; the default table can persist its projection. */
export interface ResolvedView {
  id: string
  kind: ViewRenderKind
  isDefault: boolean
  /** Ordered visible columns for any table view. */
  columns?: string[]
  /** Column widths (default table view: from settings; custom: from config). */
  columnWidths?: { [key: string]: number | undefined }
  /** Visible graph relations (custom graph view only). */
  relations?: string[]
  /** Visible card/inspector fields (custom graph view only). */
  fields?: string[]
  /** Group id to filter records/roots by, if any. */
  groupFilter?: string
}

function customViews(
  settings: EditorProjectSettings | null,
  file: string,
  type: string,
): ViewConfig[] {
  return (settings?.views?.[file]?.[type] ?? []).filter(view => !view.id.startsWith(RESERVED_VIEW_ID_PREFIX))
}

/**
 * The view tabs to show for a (file, type).
 * - All data types: default table, record and source views.
 * - Other types: default record, table, source, and supported custom views.
 * - Saved tab order applies to both built-in and custom views.
 */
export function viewTabsFor(
  settings: EditorProjectSettings | null,
  file: string,
  type: string,
  isSingleton: boolean,
  graphSupported: boolean,
): ViewTab[] {
  const order = settings?.view_order[file]?.[type] ?? []
  if (!type) {
    return [{ id: DEFAULT_SOURCE_VIEW_ID, name: '源码', kind: 'source', isDefault: true }]
  }
  const tabs: ViewTab[] = [
    { id: DEFAULT_TABLE_VIEW_ID, name: '表格', kind: 'table', isDefault: true },
    { id: DEFAULT_RECORD_VIEW_ID, name: '记录', kind: 'record', isDefault: true },
    { id: DEFAULT_SOURCE_VIEW_ID, name: '源码', kind: 'source', isDefault: true },
  ]
  if (isSingleton) return orderViewTabs(tabs, order)
  for (const view of customViews(settings, file, type)) {
    if (view.kind === 'graph' && !graphSupported) continue
    tabs.push({ id: view.id, name: view.name, kind: view.kind, isDefault: false })
  }
  return orderViewTabs(tabs, order)
}

export function orderViewTabs<T extends { id: string }>(tabs: T[], order: string[]): T[] {
  const positions = new Map(order.map((id, index) => [id, index]))
  // 新增视图排在已保存的视图之后；不可用视图不影响其余标签的相对顺序。
  return [...tabs].sort((a, b) => (positions.get(a.id) ?? order.length) - (positions.get(b.id) ?? order.length))
}

/** Resolve a viewId (default reserved id or custom uuid) to a ResolvedView.
 *  Unknown custom ids fall back to the default table view. */
export function resolveView(
  settings: EditorProjectSettings | null,
  file: string,
  type: string,
  viewId: string,
  isSingleton = false,
): ResolvedView {
  if (viewId === DEFAULT_RECORD_VIEW_ID) {
    return { id: viewId, kind: 'record', isDefault: true }
  }
  if (viewId === DEFAULT_TABLE_VIEW_ID) {
    const saved = settings?.views?.[file]?.[type]?.find(view => view.id === viewId)
    return {
      columns: saved?.columns,
      groupFilter: saved?.group_filter ?? undefined,
      id: viewId,
      kind: 'table',
      isDefault: true,
      columnWidths: settings?.default_table_column_widths?.[file]?.[type] ?? {},
    }
  }
  if (viewId === DEFAULT_SOURCE_VIEW_ID) {
    return { id: viewId, kind: 'source', isDefault: true }
  }
  const view = isSingleton ? undefined : customViews(settings, file, type).find(v => v.id === viewId)
  if (!view) {
    return resolveView(settings, file, type, DEFAULT_TABLE_VIEW_ID, isSingleton)
  }
  if (view.kind === 'table') {
    return {
      id: view.id,
      kind: 'table',
      isDefault: false,
      columns: view.columns,
      columnWidths: view.column_widths,
      groupFilter: view.group_filter ?? undefined,
    }
  }
  return {
    id: view.id,
    kind: 'graph',
    isDefault: false,
    relations: view.relations,
    fields: view.fields,
    groupFilter: view.group_filter ?? undefined,
  }
}

/** The set of fields the inspector/card should show, or undefined for "all"
 *  (default views, and table views where columns act as the visible set). */
export function visibleFieldsFor(view: ResolvedView): Set<string> | undefined {
  if (view.kind === 'table') return view.columns ? new Set(view.columns) : undefined
  if (view.kind === 'graph') return view.fields ? new Set(view.fields) : undefined
  return undefined
}

/** A predicate over record coordinates for the view's group filter. Returns a
 *  pass-all predicate when the view has no (valid) group filter. */
export function groupFilterPredicate(
  view: ResolvedView,
  groups: readonly EditorRecordGroup[],
): (coordinate: Pick<RecordCoordinate, 'actual_type' | 'id'>) => boolean {
  if (!view.groupFilter) return () => true
  const group = groups.find(g => g.id === view.groupFilter)
  if (!group) return () => true
  const members = new Set(
    group.records.map(r => `${r.actual_type}${r.id}`),
  )
  return coordinate => members.has(`${coordinate.actual_type}${coordinate.id}`)
}

let viewIdCounter = 0
/** Generate a new custom view id that never collides with reserved ids. */
export function newViewId(): string {
  viewIdCounter += 1
  const rand = Math.random().toString(36).slice(2, 10)
  return `view-${Date.now().toString(36)}-${viewIdCounter}-${rand}`
}
