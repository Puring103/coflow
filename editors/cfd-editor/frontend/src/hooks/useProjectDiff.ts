import { useCallback, useEffect, useRef, useState } from 'react'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { type GitDiffSelection } from '../components/GitDiffMode'
import { errorMessage } from '../wire'
import type { ProjectGenerationController } from '../state/editorState'
import * as api from '../api'

export function useProjectDiff(
  project: ProjectBootstrap | null,
  generation: ProjectGenerationController,
  sidebarVisible: boolean,
) {
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(false)
  const [diff, setDiff] = useState<ProjectDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selection, setSelection] = useState<GitDiffSelection>({ filePath: null, coordinate: null })
  const requestSequence = useRef(0)

  const load = useCallback(async () => {
    const request = ++requestSequence.current
    if (!project || !api.isTauri) {
      setDiff(null)
      setLoading(false)
      setError(project ? 'Git Diff 仅在桌面编辑器中可用' : '请先打开项目')
      if (!project) setSelection({ filePath: null, coordinate: null })
      return
    }
    const sessionId = project.session_id
    const revision = project.revision
    setLoading(true)
    setError(null)
    try {
      const next = await api.getProjectDiff(sessionId)
      // 编辑器 generation 与 Runtime 发布修订属于不同计数域，不能直接比较数值。
      if (requestSequence.current !== request || !generation.isCurrent(sessionId, revision)) return
      setDiff(next)
      setSelection(current => {
        const paths = new Set([
          ...next.files.map(file => file.path),
          ...next.records.map(record => record.after?.file_path ?? record.before?.file_path ?? ''),
        ])
        const coordinateStillExists = !current.coordinate || next.records.some(record => (
          record.coordinate.actual_type === current.coordinate?.actual_type
          && record.coordinate.key === current.coordinate.key
        ))
        if (current.filePath && paths.has(current.filePath) && coordinateStillExists) return current
        const record = next.records[0]
        return record
          ? { filePath: record.after?.file_path ?? record.before?.file_path ?? null, coordinate: record.coordinate }
          : { filePath: next.files[0]?.path ?? null, coordinate: null }
      })
    } catch (cause) {
      if (requestSequence.current !== request) return
      setDiff(null)
      setError(errorMessage(cause))
    } finally {
      if (requestSequence.current === request) setLoading(false)
    }
  }, [generation, project])

  const visible = sidebarVisible || active
  useEffect(() => {
    setDiff(null)
    setError(null)
    if (visible) void load()
    else requestSequence.current += 1
  }, [load, project?.revision, project?.session_id, visible])

  return {
    open,
    setOpen,
    active,
    setActive,
    diff,
    loading,
    error,
    selection,
    setSelection,
    load,
  }
}
