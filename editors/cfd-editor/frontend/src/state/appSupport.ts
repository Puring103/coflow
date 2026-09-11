import type { KeyboardEvent as ReactKeyboardEvent } from 'react'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { GraphData } from '../bindings/GraphData'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { RecordRow } from '../bindings/RecordRow'
import { coordinateId } from '../wire'
import { sameFieldPath, type CellAnchor } from './editorSelection'

const LAST_PROJECT_STORAGE_KEY = 'cfd-editor-last-project-yaml'

export function sameValueCells(left: readonly CellAnchor[], right: readonly CellAnchor[]): boolean {
  return left.length === right.length && left.every((cell, index) => {
    const other = right[index]
    return !!other
      && coordinateId(cell.coordinate) === coordinateId(other.coordinate)
      && sameFieldPath(cell.fieldPath, other.fieldPath)
  })
}

export function graphCacheKey(filePath: string, depth: number, limit: number): string {
  return `${filePath}::${depth}::${limit}`
}

export function projectGraphRows(
  cache: Record<string, GraphData>,
  revision: number,
  rows: RecordRow[],
): Record<string, GraphData> {
  const rowByCoordinate = new Map(
    rows.map(row => [`${row.coordinate.actual_type}\u001f${row.coordinate.key}`, row]),
  )
  let changed = false
  const next: Record<string, GraphData> = {}
  for (const [key, graph] of Object.entries(cache)) {
    if (graph.revision !== revision - 1 && graph.revision !== revision) {
      next[key] = graph
      continue
    }
    const nodes = graph.nodes.map(node => {
      const row = rowByCoordinate.get(`${node.coordinate.actual_type}\u001f${node.coordinate.key}`)
      if (!row) return node
      // 乐观编辑和确认回写沿用未变化节点，避免全图重建字段与端口。
      if (node.fields === row.fields && node.field_diagnostics === row.field_diagnostics
        && node.diagnostic_severity === row.diagnostic_severity) return node
      return {
        ...node,
        fields: row.fields,
        field_diagnostics: row.field_diagnostics,
        diagnostic_severity: row.diagnostic_severity,
      }
    })
    const projected = graph.revision === revision && nodes.every((node, index) => node === graph.nodes[index])
      ? graph
      : { ...graph, revision, nodes }
    if (projected !== graph) changed = true
    next[key] = projected
  }
  return changed ? next : cache
}

export function readLastProjectPath(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT_STORAGE_KEY)
  } catch {
    return null
  }
}

export function rememberLastProject(yamlPath: string): void {
  try {
    localStorage.setItem(LAST_PROJECT_STORAGE_KEY, yamlPath)
  } catch {
    // WebView 存储不可用不应阻止项目打开。
  }
}

export function projectYamlPath(directory: string): string {
  const trimmed = directory.replace(/[\\/]+$/, '')
  const separator = trimmed.includes('\\') ? '\\' : '/'
  return `${trimmed}${separator}coflow.yaml`
}

export function definedColumnWidths(
  widths: { [column: string]: number | undefined } | undefined,
): Record<string, number> | undefined {
  if (!widths) return undefined
  return Object.fromEntries(
    Object.entries(widths).filter((entry): entry is [string, number] => entry[1] !== undefined),
  )
}

export function collectSourceFiles(bootstrap: ProjectBootstrap): string[] {
  const paths: string[] = []
  const walk = (node: ProjectBootstrap['file_tree'][number]) => {
    if (!node.is_dir && node.in_sources) paths.push(node.path)
    for (const child of node.children) walk(child)
  }
  for (const node of bootstrap.file_tree) walk(node)
  return paths
}

export function dimensionForFile(
  dimensions: DimensionInfo[],
  filePath: string,
): DimensionInfo | undefined {
  const normalizedFile = filePath.replace(/\\/g, '/')
  return dimensions.find(dimension => {
    if (!dimension.out_dir) return false
    const directory = dimension.out_dir.replace(/\\/g, '/').replace(/\/+$/, '')
    return normalizedFile.startsWith(`${directory}/`)
  })
}

export function onToolbarKeyDown(
  event: ReactKeyboardEvent,
  onExitDown: () => void,
): void {
  if (!(event.target instanceof HTMLButtonElement)) return
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    onExitDown()
    return
  }
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
  const buttons = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled):not(.tab-view)'),
  )
  const index = buttons.indexOf(event.target)
  if (index < 0) return
  event.preventDefault()
  if (event.key === 'Home') buttons[0]?.focus()
  else if (event.key === 'End') buttons[buttons.length - 1]?.focus()
  else {
    const next = index + (event.key === 'ArrowRight' ? 1 : -1)
    if (next >= 0 && next < buttons.length) buttons[next].focus()
    else if (next < 0) onExitDown()
  }
}
