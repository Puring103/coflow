import { useCallback, type Dispatch, type MutableRefObject, type SetStateAction } from 'react'
import * as api from '../api'
import type { FileRecords } from '../bindings/FileRecords'
import type { FlatDiagnostic } from '../bindings/FlatDiagnostic'
import type { GraphData } from '../bindings/GraphData'
import { queryClient } from '../queryClient'
import { editorQueryKeys } from '../queryKeys'
import { graphCacheKey, projectGraphRows } from '../state/appSupport'
import {
  publishMutationGeneration,
  type MutationPublicationRequest,
  type ProjectGenerationController,
} from '../state/editorState'

interface MutationPublicationOptions {
  generation: ProjectGenerationController
  graphDepth: number
  graphLimit: number
  graphCacheRef: MutableRefObject<Record<string, GraphData>>
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
  graphDepth,
  graphLimit,
  graphCacheRef,
  setFiles,
  setGraphs,
  acceptRevision,
}: MutationPublicationOptions) {
  return useCallback((request: MutationPublicationRequest) => publishMutationGeneration({
    acceptRevision,
    isCurrent: (sessionId, revision) => generation.isCurrent(sessionId, revision),
    getFileRecords: api.getFileRecords,
    publishFileRecords: records => {
      const identity = generation.currentIdentity()
      if (identity) {
        for (const [file, fileRecords] of records) {
          queryClient.setQueryData(
            editorQueryKeys.fileRecords(identity.sessionId, identity.revision, file),
            fileRecords,
          )
        }
      }
      setFiles(current => {
        const next = { ...current }
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
      const identity = generation.currentIdentity()
      if (identity?.revision !== revision) return
      for (const recordsForFile of records) {
        const graph = next[graphCacheKey(recordsForFile.file_path, graphDepth, graphLimit)]
        if (graph) {
          queryClient.setQueryData(
            editorQueryKeys.graph(
              identity.sessionId,
              revision,
              recordsForFile.file_path,
              graphDepth,
              graphLimit,
            ),
            graph,
          )
        }
      }
    },
  }, request), [acceptRevision, generation, graphCacheRef, graphDepth, graphLimit, setFiles, setGraphs])
}
