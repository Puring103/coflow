import { lazy } from 'react'

// 预加载与 React.lazy 共享同一 Promise，切换视图时复用已经加载的模块。
function cachedViewModule<T>(loader: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | undefined
  return () => pending ??= loader()
}
const viewModules = {
  graph: cachedViewModule(() => import('../features/graph/components/GraphView').then(module => ({ default: module.GraphView }))),
  source: cachedViewModule(() => import('../features/source-editor/components/SourceEditorView').then(module => ({ default: module.SourceEditorView }))),
  table: cachedViewModule(() => import('../features/table/components/TableView').then(module => ({ default: module.TableView }))),
  record: cachedViewModule(() => import('../features/record-editing/components/RecordView').then(module => ({ default: module.RecordView }))),
  dimension: cachedViewModule(() => import('../features/table/components/DimensionTableView').then(module => ({ default: module.DimensionTableView }))),
  diff: cachedViewModule(() => import('../features/git-diff/components/GitDiffMode').then(module => ({ default: module.GitDiffMode }))),
}
export function preloadView(kind: string) {
  if (kind in viewModules) void viewModules[kind as keyof typeof viewModules]().catch(() => { /* 实际视图接管加载失败。 */ })
}
export const GraphView = lazy(viewModules.graph)
export const SourceEditorView = lazy(viewModules.source)
export const TableView = lazy(viewModules.table)
export const RecordView = lazy(viewModules.record)
export const DimensionTableView = lazy(viewModules.dimension)
export const GitDiffMode = lazy(viewModules.diff)
export const GitDiffSidebar = lazy(() => import('../features/git-diff/components/GitDiffMode').then(module => ({ default: module.GitDiffSidebar })))

export const ViewLoading = () => <div className="content-empty" role="status"><div className="content-empty-title">加载视图中…</div></div>
