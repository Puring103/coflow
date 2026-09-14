import { listen } from '@tauri-apps/api/event'
import type { ProjectReloadedEvent } from '../bindings/ProjectReloadedEvent'
import type { ProjectWatchErrorEvent } from '../bindings/ProjectWatchErrorEvent'
import { fromIpc } from '../wire'

/** Tauri 事件名唯一位置：与核心 `PROJECT_*_EVENT` 对齐，前端禁止手写字面量。 */
export const PROJECT_RELOADED_EVENT = 'project_reloaded'
export const PROJECT_WATCH_ERROR_EVENT = 'project_watch_error'

export async function onProjectReloaded(handler: (event: ProjectReloadedEvent) => void): Promise<() => void> {
  return listen<ProjectReloadedEvent>(PROJECT_RELOADED_EVENT, event => handler(fromIpc(event.payload) as ProjectReloadedEvent))
}

export async function onProjectWatchError(handler: (event: ProjectWatchErrorEvent) => void): Promise<() => void> {
  return listen<ProjectWatchErrorEvent>(PROJECT_WATCH_ERROR_EVENT, event => handler(fromIpc(event.payload) as ProjectWatchErrorEvent))
}
