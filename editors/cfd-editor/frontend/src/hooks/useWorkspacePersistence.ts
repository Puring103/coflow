import { useEffect, useRef, type Dispatch, type SetStateAction } from 'react'
import * as api from '../api'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { ProjectGenerationController } from '../state/editorState'
import { workspaceToWire, type WorkspaceTab } from '../state/workspaceTabs'
import { errorMessage } from '../wire'

interface WorkspacePersistenceOptions {
  project: ProjectBootstrap | null
  readySessionId: number | null
  tabs: WorkspaceTab[]
  activeTabId: string | null
  generation: ProjectGenerationController
  setSettings: Dispatch<SetStateAction<EditorProjectSettings | null>>
  setError: Dispatch<SetStateAction<string | null>>
}

export function useWorkspacePersistence({
  project,
  readySessionId,
  tabs,
  activeTabId,
  generation,
  setSettings,
  setError,
}: WorkspacePersistenceOptions): void {
  const saveChain = useRef<Promise<void>>(Promise.resolve())

  useEffect(() => {
    if (!api.isTauri || !project || readySessionId !== project.session_id) return
    const sessionId = project.session_id
    const workspace = workspaceToWire(tabs, activeTabId)
    const timer = window.setTimeout(() => {
      // 连续标签操作串行落盘，后写入的状态不会被较早请求覆盖。
      saveChain.current = saveChain.current
        .catch(() => undefined)
        .then(async () => {
          if (generation.currentSession() !== sessionId) return
          const saved = await api.setWorkspace(sessionId, workspace)
          if (generation.currentSession() !== sessionId) return
          setSettings(current => current ? { ...current, workspace: saved.workspace } : current)
        })
        .catch(error => {
          if (generation.currentSession() === sessionId) {
            setError(`保存工作区状态失败: ${errorMessage(error)}`)
          }
        })
    }, 250)
    return () => window.clearTimeout(timer)
  }, [activeTabId, generation, project?.session_id, readySessionId, setError, setSettings, tabs])
}
