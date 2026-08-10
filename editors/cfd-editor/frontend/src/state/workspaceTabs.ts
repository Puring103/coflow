import type { FileTypeOption } from '../bindings/FileTypeOption'
import type { EditorWorkspaceState } from '../bindings/EditorWorkspaceState'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { Route } from '../wire'
import {
  DEFAULT_RECORD_VIEW_ID,
  DEFAULT_TABLE_VIEW_ID,
  type ViewRenderKind,
} from './views'

export interface WorkspaceTab {
  id: string
  filePath: string
  typeName: string
  viewId: string
  viewKind: ViewRenderKind
  coordinate?: RecordCoordinate
}

export interface ProjectWorkspace {
  tabs: WorkspaceTab[]
  activeTabId: string | null
}

export function workspaceTabId(filePath: string, typeName: string): string {
  return `${filePath}\u001f${typeName}`
}

export function defaultWorkspaceTab(
  filePath: string,
  typeName: string,
  isSingleton: boolean,
): WorkspaceTab {
  return {
    id: workspaceTabId(filePath, typeName),
    filePath,
    typeName,
    viewId: isSingleton ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID,
    viewKind: isSingleton ? 'record' : 'table',
  }
}

export function routeForWorkspaceTab(
  tab: WorkspaceTab,
  fallbackCoordinate?: RecordCoordinate,
): Route {
  if (tab.viewKind === 'record') {
    const coordinate = isCoordinate(tab.coordinate, tab.typeName)
      ? tab.coordinate
      : isCoordinate(fallbackCoordinate, tab.typeName) ? fallbackCoordinate : undefined
    if (!coordinate) {
      return {
        view: 'table',
        file: tab.filePath,
        viewId: DEFAULT_TABLE_VIEW_ID,
        typeFilter: tab.typeName,
      }
    }
    return {
      view: 'record',
      file: tab.filePath,
      viewId: tab.viewId,
      coordinate,
    }
  }
  return {
    view: tab.viewKind,
    file: tab.filePath,
    viewId: tab.viewId,
    typeFilter: tab.typeName,
  }
}

export function sanitizeProjectWorkspace(
  value: unknown,
  fileTypes: { [file: string]: FileTypeOption[] | undefined },
  sourceFiles?: ReadonlySet<string>,
): ProjectWorkspace | null {
  if (!isObject(value) || !Array.isArray(value.tabs)) return null
  const tabs: WorkspaceTab[] = []
  const seen = new Set<string>()
  for (const candidate of value.tabs) {
    if (!isObject(candidate)) continue
    const filePath = stringProperty(candidate, 'file_path', 'filePath')
    const typeName = stringProperty(candidate, 'type_name', 'typeName')
    const option = fileTypes[filePath]?.find(type => type.name === typeName)
    const isDimensionFile = !typeName && (sourceFiles?.has(filePath) ?? false)
    if (!option && !isDimensionFile) continue
    const id = workspaceTabId(filePath, typeName)
    if (seen.has(id)) continue
    seen.add(id)

    const rawKind = candidate.view_kind ?? candidate.viewKind
    const requestedKind = isViewKind(rawKind) ? rawKind : 'table'
    const viewKind: ViewRenderKind = option?.is_singleton
      ? 'record'
      : isDimensionFile && requestedKind === 'graph' ? 'table' : requestedKind
    const requestedId = stringProperty(candidate, 'view_id', 'viewId')
    const viewId = option?.is_singleton
      ? DEFAULT_RECORD_VIEW_ID
      : requestedId || (viewKind === 'record' ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID)
    const coordinate = viewKind === 'record' && isCoordinate(candidate.coordinate, typeName)
      ? candidate.coordinate
      : undefined
    tabs.push({ id, filePath, typeName, viewId, viewKind, coordinate })
  }
  if (tabs.length === 0) return null
  const requestedActiveId = typeof value.active_tab_id === 'string'
    ? value.active_tab_id
    : typeof value.activeTabId === 'string' ? value.activeTabId : null
  const activeTabId = requestedActiveId && tabs.some(tab => tab.id === requestedActiveId)
    ? requestedActiveId
    : tabs[0].id
  return { tabs, activeTabId }
}

export function workspaceToWire(
  tabs: readonly WorkspaceTab[],
  activeTabId: string | null,
): EditorWorkspaceState {
  return {
    tabs: tabs.map(tab => ({
      file_path: tab.filePath,
      type_name: tab.typeName,
      view_id: tab.viewId,
      view_kind: tab.viewKind,
      ...(isCoordinate(tab.coordinate, tab.typeName) ? { coordinate: tab.coordinate } : {}),
    })),
    active_tab_id: activeTabId,
  }
}

function isViewKind(value: unknown): value is ViewRenderKind {
  return value === 'record' || value === 'table' || value === 'graph'
}

function isCoordinate(value: unknown, typeName: string): value is RecordCoordinate {
  return isObject(value)
    && value.actual_type === typeName
    && typeof value.key === 'string'
    && value.key.length > 0
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function stringProperty(
  value: Record<string, unknown>,
  wireName: string,
  legacyName: string,
): string {
  const candidate = value[wireName] ?? value[legacyName]
  return typeof candidate === 'string' ? candidate : ''
}
