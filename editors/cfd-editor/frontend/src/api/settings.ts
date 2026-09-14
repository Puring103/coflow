import type { DimensionFileRecords } from '../bindings/DimensionFileRecords'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { EditorWorkspaceState } from '../bindings/EditorWorkspaceState'
import type { ViewConfig } from '../bindings/ViewConfig'
import { invokeCommand } from './tauriEnv'

export async function getProjectSettings(sessionId: number): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('get_project_settings', { sessionId })
}

export async function getDimensionFileRecords(
  sessionId: number,
  filePath: string,
): Promise<DimensionFileRecords> {
  return invokeCommand<DimensionFileRecords>('get_dimension_file_records', { sessionId, filePath })
}

export async function setDefaultTableColumnWidths(
  sessionId: number,
  filePath: string,
  actualType: string,
  widths: Record<string, number>,
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_default_table_column_widths', {
    sessionId,
    filePath,
    actualType,
    widths,
  })
}

export async function setGraphPositions(
  sessionId: number,
  viewKey: string,
  positions: Record<string, [number, number]>,
): Promise<void> {
  return invokeCommand<void>('set_graph_positions', { sessionId, viewKey, positions })
}

export async function setViewColumnWidths(
  sessionId: number,
  filePath: string,
  actualType: string,
  viewId: string,
  widths: Record<string, number>,
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_view_column_widths', {
    sessionId,
    filePath,
    actualType,
    viewId,
    widths,
  })
}

export async function setRecordGroups(
  sessionId: number,
  filePath: string,
  actualType: string,
  groups: EditorRecordGroup[],
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_record_groups', {
    sessionId,
    filePath,
    actualType,
    groups,
  })
}

export async function setViewOrder(
  sessionId: number,
  filePath: string,
  actualType: string,
  order: string[],
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_view_order', { sessionId, filePath, actualType, order })
}

export async function setShortNameField(sessionId: number, actualType: string, field: string | null): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_short_name_field', { sessionId, actualType, field })
}

export async function setViews(
  sessionId: number,
  filePath: string,
  actualType: string,
  views: ViewConfig[],
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_views', {
    sessionId,
    filePath,
    actualType,
    views,
  })
}

export async function setWorkspace(
  sessionId: number,
  workspace: EditorWorkspaceState,
): Promise<EditorProjectSettings> {
  return invokeCommand<EditorProjectSettings>('set_workspace', { sessionId, workspace })
}

