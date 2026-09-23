import { dimensionTargetId, type DimensionTarget } from './dimensionNavigation'
import type { FileTypeOption } from '../bindings/FileTypeOption'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { EditorWorkspaceState } from '../bindings/EditorWorkspaceState'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { Route } from '../wire'
import {
  DEFAULT_RECORD_VIEW_ID,
  DEFAULT_SOURCE_VIEW_ID,
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
  dimensionTarget?: DimensionTarget
}

export interface ProjectWorkspace {
  tabs: WorkspaceTab[]
  activeTabId: string | null
}

export function workspaceTabId(filePath: string, typeName: string): string {
  return `${filePath}\u001f${typeName}`
}

export function dimensionWorkspaceTab(target: DimensionTarget): WorkspaceTab {
  const filePath = `@dimension/${target.dimension}`
  return { id: dimensionTargetId(target), filePath, typeName: '',
    viewId: target.singleton ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID,
    viewKind: target.singleton ? 'record' : 'table', dimensionTarget: target }
}

export function defaultWorkspaceTab(
  filePath: string,
  typeName: string,
  isSingleton: boolean,
): WorkspaceTab {
  const sourceOnly = filePath.endsWith('.cft')
  return {
    id: workspaceTabId(filePath, typeName),
    filePath,
    typeName,
    viewId: sourceOnly ? DEFAULT_SOURCE_VIEW_ID : isSingleton ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID,
    viewKind: sourceOnly ? 'source' : isSingleton ? 'record' : 'table',
  }
}

export function routeForWorkspaceTab(
  tab: WorkspaceTab,
  fallbackCoordinate?: RecordCoordinate,
): Route {
  if (tab.dimensionTarget) {
    return { view: 'table', file: tab.filePath, viewId: DEFAULT_TABLE_VIEW_ID, typeFilter: '', dimensionTargetId: tab.id }
  }
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

export function workspaceTabWithView(
  tab: WorkspaceTab,
  viewKind: ViewRenderKind,
  viewId: string,
  coordinate?: RecordCoordinate,
): WorkspaceTab {
  return {
    ...tab,
    viewKind,
    viewId,
    coordinate: coordinate ?? tab.coordinate,
  }
}

export function sanitizeProjectWorkspace(
  value: unknown,
  fileTypes: { [file: string]: FileTypeOption[] | undefined },
  sourceFiles?: ReadonlySet<string>,
  dimensions: DimensionInfo[] = [],
  dimensionFiles: ReadonlySet<string> = new Set(),
): ProjectWorkspace | null {
  if (!isObject(value) || !Array.isArray(value.tabs)) return null
  const tabs: WorkspaceTab[] = []
  const seen = new Set<string>()
  for (const candidate of value.tabs) {
    if (!isObject(candidate)) continue
    const filePath = stringProperty(candidate, 'file_path')
    const typeName = stringProperty(candidate, 'type_name')
    const target = isObject(candidate.dimension_target) ? readDimensionTarget(candidate.dimension_target) : null
    const dimension = target && dimensions.find(item => item.name === target.dimension)
    const validTarget = target && dimension && filePath === `@dimension/${target.dimension}`
      && dimensionFiles.has(target.ownerFile) && (fileTypes[target.ownerFile] ?? [])
        .some(item => item.name === target.typeName && item.record_count > 0
          && item.is_singleton === target.singleton
          && (target.singleton ? (item.dimension_fields[target.dimension] ?? []).length > 0
            : (item.dimension_fields[target.dimension] ?? []).includes(target.field!))) ? target : null
    const option = fileTypes[filePath]?.find(type => type.name === typeName)
    const isDimensionFile = !target && !typeName && (sourceFiles?.has(filePath) ?? false)
    if (!option && !isDimensionFile && !validTarget) continue
    const id = validTarget ? dimensionTargetId(validTarget) : workspaceTabId(filePath, typeName)
    if (seen.has(id)) continue
    seen.add(id)

    const rawKind = candidate.view_kind
    const requestedKind = isViewKind(rawKind) ? rawKind : 'table'
    const requestedId = stringProperty(candidate, 'view_id')
    const pluginView = requestedId.includes('/')
    const viewKind: ViewRenderKind = validTarget ? (validTarget.singleton ? 'record' : requestedKind === 'record' ? 'record' : 'table') : pluginView
      ? 'table'
      : option?.is_singleton
      ? requestedKind === 'source' ? 'source' : 'record'
      : isDimensionFile && requestedKind === 'graph' ? 'table' : requestedKind
    const viewId = validTarget ? (validTarget.singleton || viewKind === 'record' ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID) : pluginView
      ? requestedId
      : option?.is_singleton
      ? viewKind === 'source' ? DEFAULT_SOURCE_VIEW_ID : DEFAULT_RECORD_VIEW_ID
      : requestedId || (viewKind === 'record'
        ? DEFAULT_RECORD_VIEW_ID
        : viewKind === 'source' ? DEFAULT_SOURCE_VIEW_ID : DEFAULT_TABLE_VIEW_ID)
    const coordinate = viewKind === 'record' && isCoordinate(candidate.coordinate, typeName)
      ? candidate.coordinate
      : undefined
    tabs.push({ id, filePath, typeName, viewId, viewKind, coordinate, dimensionTarget: validTarget ?? undefined })
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
      coordinate: isCoordinate(tab.coordinate, tab.typeName) ? tab.coordinate : null,
      dimension_target: tab.dimensionTarget ?? null,
    })),
    active_tab_id: activeTabId,
  }
}

function isViewKind(value: unknown): value is ViewRenderKind {
  return value === 'record' || value === 'table' || value === 'graph' || value === 'source'
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
): string {
  const candidate = value[wireName]
  return typeof candidate === 'string' ? candidate : ''
}

function readDimensionTarget(value: Record<string, unknown>): DimensionTarget | null {
  const dimension = stringProperty(value, 'dimension')
  const ownerFile = stringProperty(value, 'ownerFile')
  const typeName = stringProperty(value, 'typeName')
  const field = value.field
  const singleton = value.singleton
  if (!dimension || !ownerFile || !typeName || typeof singleton !== 'boolean'
    || !(typeof field === 'string' || field === null)
    || (singleton ? field !== null : !field)) return null
  return { dimension, ownerFile, typeName, field, singleton }
}
