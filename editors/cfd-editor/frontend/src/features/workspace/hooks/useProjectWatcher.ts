import { useEffect } from 'react'
import * as api from '../../../api'
import type { ProjectBootstrap } from '../../../bindings/ProjectBootstrap'
import type { ProjectGenerationController } from '../state/editorState'

interface ProjectWatcherOptions {
  project: ProjectBootstrap | null
  generation: ProjectGenerationController
  refresh: (bootstrap: ProjectBootstrap) => Promise<void>
  reportError: (sessionId: number, prefix: string, error: unknown) => void
}

export function useProjectWatcher({
  project,
  generation,
  refresh,
  reportError,
}: ProjectWatcherOptions): void {
  useEffect(() => {
    if (!api.isTauri || !project) return
    const sessionId = project.session_id
    let disposed = false
    let unlistenChanged: (() => void) | null = null
    let unlistenError: (() => void) | null = null
    const isCurrent = () => !disposed && generation.currentSession() === sessionId

    api.onProjectReloaded(event => {
      if (!isCurrent() || event.session_id !== sessionId) return
      // 后端已经提交刷新结果，直接消费该代次，避免再次重建会话。
      refresh(event.bootstrap)
        .catch(error => {
          if (isCurrent()) reportError(sessionId, '刷新项目失败', error)
        })
    }).then(unlisten => {
      if (isCurrent()) unlistenChanged = unlisten
      else unlisten()
    }).catch(error => {
      if (isCurrent()) reportError(sessionId, '监听项目变更失败', error)
    })

    api.onProjectWatchError(event => {
      if (isCurrent() && event.session_id === sessionId) reportError(sessionId, '监听项目变更失败', event.error)
    }).then(unlisten => {
      if (isCurrent()) unlistenError = unlisten
      else unlisten()
    }).catch(error => {
      if (isCurrent()) reportError(sessionId, '监听项目变更失败', error)
    })

    return () => {
      disposed = true
      unlistenChanged?.()
      unlistenError?.()
    }
  }, [generation, project?.session_id, refresh, reportError])
}
