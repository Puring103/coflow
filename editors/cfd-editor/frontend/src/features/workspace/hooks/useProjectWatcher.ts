import { useEffect, useEffectEvent } from 'react'
import * as api from '../../../api'
import type { ProjectBootstrap } from '../../../bindings/ProjectBootstrap'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'
import type { ProjectGenerationController } from '../state/editorState'

interface ProjectWatcherOptions {
  project: ProjectBootstrap | null
  generation: ProjectGenerationController
  refresh: (bootstrap: ProjectBootstrap) => Promise<void>
  refreshSettings: (settings: EditorProjectSettings) => void
  reportError: (sessionId: number, prefix: string, error: unknown) => void
}

export function useProjectWatcher({
  project,
  generation,
  refresh,
  refreshSettings,
  reportError,
}: ProjectWatcherOptions): void {
  const onReload = useEffectEvent(refresh)
  const onSettings = useEffectEvent(refreshSettings)
  useEffect(() => {
    if (!api.isTauri || !project) return
    const sessionId = project.session_id
    let disposed = false
    let unlistenChanged: (() => void) | null = null
    let unlistenError: (() => void) | null = null
    let unlistenSettings: (() => void) | null = null
    let settingsRequest = 0
    const isCurrent = () => !disposed && generation.currentSession() === sessionId

    api.onProjectReloaded(event => {
      if (!isCurrent() || event.session_id !== sessionId) return
      // 后端已经提交刷新结果，直接消费该代次，避免再次重建会话。
      onReload(event.bootstrap)
        .catch(error => {
          if (isCurrent()) reportError(sessionId, '刷新项目失败', error)
        })
    }).then(unlisten => {
      if (isCurrent()) unlistenChanged = unlisten
      else unlisten()
    }).catch(error => {
      if (isCurrent()) reportError(sessionId, '监听项目变更失败', error)
    })

    api.onProjectSettingsChanged(changedSession => {
      if (!isCurrent() || changedSession !== sessionId) return
      // 只发布最新一次读取结果，忽略上一项目或较早变更的响应。
      const request = ++settingsRequest
      void api.getProjectSettings(sessionId).then(settings => {
        if (isCurrent() && request === settingsRequest) onSettings(settings)
      }).catch(error => {
        if (isCurrent() && request === settingsRequest) reportError(sessionId, '刷新编辑器设置失败', error)
      })
    }).then(unlisten => {
      if (isCurrent()) unlistenSettings = unlisten
      else unlisten()
    }).catch(error => {
      if (isCurrent()) reportError(sessionId, '监听编辑器设置失败', error)
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
      unlistenSettings?.()
    }
  }, [generation, project?.session_id, reportError])
}
