import { useCallback, useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { ProjectDiff } from '../bindings/ProjectDiff'
import { type GitDiffSelection } from '../components/GitDiffMode'
import { errorMessage } from '../wire'
import type { ProjectGenerationController } from '../state/editorState'
import * as api from '../api'
import { editorQueryKeys } from '../queryKeys'

export function useProjectDiff(
  project: ProjectBootstrap | null,
  generation: ProjectGenerationController,
  sidebarVisible: boolean,
) {
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(false)
  const [selection, setSelection] = useState<GitDiffSelection>({ filePath: null, typeName: null, coordinate: null })
  const visible = sidebarVisible || active
  const query = useQuery({
    queryKey: editorQueryKeys.projectDiff(project?.session_id, project?.revision),
    enabled: visible && api.isTauri && project !== null,
    queryFn: async (): Promise<ProjectDiff> => {
      if (!project) throw new Error('请先打开项目')
      const next = await api.getProjectDiff(project.session_id)
      if (!generation.isCurrent(project.session_id, project.revision)) {
        throw new Error('项目已刷新')
      }
      return next
    },
  })

  const load = useCallback(async () => {
    if (!project || !api.isTauri) return
    await query.refetch()
  }, [project, query.refetch])

  useEffect(() => {
    const next = query.data
    if (!next) {
      if (!project) setSelection({ filePath: null, typeName: null, coordinate: null })
      return
    }
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
      if (record) {
        const filePath = record.after?.file_path ?? record.before?.file_path ?? null
        return { filePath, typeName: record.coordinate.actual_type, coordinate: record.coordinate }
      }
      return { filePath: next.files[0]?.path ?? null, typeName: null, coordinate: null }
    })
  }, [project, query.data])

  return {
    open,
    setOpen,
    active,
    setActive,
    diff: query.data ?? null,
    loading: query.isFetching,
    error: !project
      ? '请先打开项目'
      : !api.isTauri
        ? 'Git Diff 仅在桌面编辑器中可用'
        : query.error ? errorMessage(query.error) : null,
    selection,
    setSelection,
    load,
  }
}
