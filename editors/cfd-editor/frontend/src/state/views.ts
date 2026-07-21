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
export const DEFAULT_GRAPH_VIEW_ID = '__default_graph'
export const RESERVED_VIEW_ID_PREFIX = '__'

export type ViewRenderKind = 'record' | 'table' | 'graph'

/** A tab in the view-tab row. */
export interface ViewTab {
  id: string
  name: string
  kind: ViewRenderKind
  isDefault: boolean
}

/** A view resolved to everything the renderer needs. Default views resolve
 *  with `isDefault: true` and no field/relation restrictions. */
export interface ResolvedView {
  id: string
  kind: ViewRenderKind
  isDefault: boolean
  /** Ordered visible columns (custom table view only). */
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
  return settings?.views?.[file]?.[type] ?? []
}

/**
 * The view tabs to show for a (file, type).
 * - Singleton types: only the default record view (no list, no table/graph).
 * - Otherwise: default record + default table, then default graph if the
 *   records support it, then custom views in stored order.
 */
export function viewTabsFor(
  settings: EditorProjectSettings | null,
  file: string,
  type: string,
  isSingleton: boolean,
  graphSupported: boolean,
): ViewTab[] {
  if (isSingleton) {
    return [{ id: DEFAULT_RECORD_VIEW_ID, name: '记录', kind: 'record', isDefault: true }]
  }
  const tabs: ViewTab[] = [
    { id: DEFAULT_RECORD_VIEW_ID, name: '记录', kind: 'record', isDefault: true },
    { id: DEFAULT_TABLE_VIEW_ID, name: '表格', kind: 'table', isDefault: true },
  ]
  if (graphSupported) {
    tabs.push({ id: DEFAULT_GRAPH_VIEW_ID, name: '图谱', kind: 'graph', isDefault: true })
  }
  for (const view of customViews(settings, file, type)) {
    if (view.kind === 'graph' && !graphSupported) continue
    tabs.push({ id: view.id, name: view.name, kind: view.kind, isDefault: false })
  }
  return tabs
}

/** Resolve a viewId (default reserved id or custom uuid) to a ResolvedView.
 *  Unknown custom ids fall back to the default table view. */
export function resolveView(
  settings: EditorProjectSettings | null,
  file: string,
  type: string,
  viewId: string,
): ResolvedView {
  if (viewId === DEFAULT_RECORD_VIEW_ID) {
    return { id: viewId, kind: 'record', isDefault: true }
  }
  if (viewId === DEFAULT_TABLE_VIEW_ID) {
    return {
      id: viewId,
      kind: 'table',
      isDefault: true,
      columnWidths: settings?.default_table_column_widths?.[file]?.[type] ?? {},
    }
  }
  if (viewId === DEFAULT_GRAPH_VIEW_ID) {
    return { id: viewId, kind: 'graph', isDefault: true }
  }
  const view = customViews(settings, file, type).find(v => v.id === viewId)
  if (!view) {
    return {
      id: DEFAULT_TABLE_VIEW_ID,
      kind: 'table',
      isDefault: true,
      columnWidths: settings?.default_table_column_widths?.[file]?.[type] ?? {},
    }
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
  if (view.isDefault) return undefined
  if (view.kind === 'table') return view.columns ? new Set(view.columns) : undefined
  if (view.kind === 'graph') return view.fields ? new Set(view.fields) : undefined
  return undefined
}

/** A predicate over record coordinates for the view's group filter. Returns a
 *  pass-all predicate when the view has no (valid) group filter. */
export function groupFilterPredicate(
  view: ResolvedView,
  groups: readonly EditorRecordGroup[],
): (coordinate: Pick<RecordCoordinate, 'actual_type' | 'key'>) => boolean {
  if (!view.groupFilter) return () => true
  const group = groups.find(g => g.id === view.groupFilter)
  if (!group) return () => true
  const members = new Set(
    group.records.map(r => `${r.actual_type}${r.key}`),
  )
  return coordinate => members.has(`${coordinate.actual_type}${coordinate.key}`)
}

let viewIdCounter = 0
/** Generate a new custom view id that never collides with reserved ids. */
export function newViewId(): string {
  viewIdCounter += 1
  const rand = Math.random().toString(36).slice(2, 10)
  return `view-${Date.now().toString(36)}-${viewIdCounter}-${rand}`
}
