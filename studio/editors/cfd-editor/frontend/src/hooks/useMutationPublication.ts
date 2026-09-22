import { useCallback, type Dispatch, type MutableRefObject, type SetStateAction } from 'react'
import * as api from '../api'
import type { FileRecords } from '../bindings/FileRecords'
import type { FlatDiagnostic } from '../bindings/FlatDiagnostic'
import type { GraphData } from '../bindings/GraphData'
import { projectGraphRows } from '../state/appSupport'
import {
  publishMutationGeneration,
  type MutationPublicationRequest,
  type ProjectGenerationController,
} from '../state/editorState'

interface MutationPublicationOptions {
  generation: ProjectGenerationController
  graphCacheRef: MutableRefObject<Record<string, GraphData>>
  fileCacheRef: MutableRefObject<Record<string, FileRecords>>
  setFiles: Dispatch<SetStateAction<Record<string, FileRecords>>>
  setGraphs: Dispatch<SetStateAction<Record<string, GraphData>>>
  acceptRevision: (
    sessionId: number,
    revision: number,
    diagnostics: FlatDiagnostic[],
  ) => boolean
}

export function useMutationPublication({
  generation,
  graphCacheRef,
  fileCacheRef,
  setFiles,
  setGraphs,
  acceptRevision,
}: MutationPublicationOptions) {
  return useCallback((request: MutationPublicationRequest) => publishMutationGeneration({
    acceptRevision,
    isCurrent: (sessionId, revision) => generation.isCurrent(sessionId, revision),
    getFileRecords: api.getFileRecords,
    cachedFileRecords: file => fileCacheRef.current[file],
    publishFileRecords: records => {
      setFiles(current => {
        const next = { ...current }
        // 全局版本控制提交顺序；没有进入变更集的文件直接沿用记录对象。
        const changedFiles = new Set(request.changes.files.map(patch => patch.data.file_path))
        for (const [file, previous] of Object.entries(current)) {
          if (previous.revision === request.changes.base_revision && !changedFiles.has(file)) {
            next[file] = { ...previous, revision: request.revision }
          }
        }
        for (const [file, fileRecords] of records) next[file] = fileRecords
        return next
      })
    },
    publishGraphProjection: (revision, records, topologyChanged) => {
      if (topologyChanged) return
      const next = projectGraphRows(
        graphCacheRef.current,
        revision,
        records.flatMap(file => file.records),
      )
      setGraphs(next)

    },
  }, request), [acceptRevision, generation, fileCacheRef, graphCacheRef, setFiles, setGraphs])
}
