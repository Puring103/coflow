import type { WorkspaceViewKind } from '../bindings/WorkspaceViewKind'

/** 前端路由：以后端 `WorkspaceViewKind` 为准，字符串只在路由层出现一次。 */
export type Route =
  | { view: 'table'; file: string; viewId: string; typeFilter?: string }
  | { view: 'record'; file: string; viewId: string; coordinate: import('../bindings/RecordCoordinate').RecordCoordinate }
  | { view: 'graph'; file: string; viewId: string; typeFilter?: string }
  | { view: 'source'; file: string; viewId: string; typeFilter?: string }

const VIEW_KIND_TO_ROUTE: Record<WorkspaceViewKind, Route['view']> = {
  record: 'record',
  table: 'table',
  graph: 'graph',
  source: 'source',
}

export function routeViewFromWorkspaceKind(kind: WorkspaceViewKind): Route['view'] {
  return VIEW_KIND_TO_ROUTE[kind]
}
