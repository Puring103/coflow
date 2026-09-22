import { useQuery } from '@tanstack/react-query'
import type { DimensionInfo } from '../bindings/DimensionInfo'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { ProjectGenerationController } from '../state/editorState'
import { dimensionForFile, graphCacheKey } from '../state/appSupport'
import * as api from '../api'
import { MOCK_DIMENSION_FILE_RECORDS, MOCK_GRAPH } from '../mock'
import { editorQueryKeys } from '../queryKeys'
import { validateQueryRevision } from '../editorQueries'

export interface EditorDataRoute {
  file: string
  view: string
}

export function useEditorDataQueries(
  project: ProjectBootstrap | null,
  dimensions: DimensionInfo[],
  route: EditorDataRoute | null,
  generation: ProjectGenerationController,
  graphDepth: number,
  graphLimit: number,
) {
  const file = route?.file ?? ''
  const revision = project?.revision ?? 0
  const sessionId = project?.session_id ?? 0
  const dimension = project && file ? dimensionForFile(dimensions, file) : null
  const isDataFile = !!project && !!route && !file.endsWith('.cft') && !dimension

  const validateRevision = <T extends { revision: number }>(value: T): T => {
    if (!generation.isCurrent(sessionId, revision)) throw new Error('项目已刷新')
    return validateQueryRevision(value, revision)
  }

  const fileQuery = useQuery({
    queryKey: editorQueryKeys.fileRecords(sessionId, revision, file),
    enabled: isDataFile && api.isTauri,
    queryFn: async () => validateRevision(await api.getFileRecords(sessionId, file)),
  })
  const dimensionQuery = useQuery({
    queryKey: editorQueryKeys.dimensionRecords(sessionId, revision, file),
    enabled: !!project && !!route && !!dimension && api.isTauri,
    queryFn: async () => validateRevision(await api.getDimensionFileRecords(sessionId, file)),
  })
  const graphQuery = useQuery({
    queryKey: editorQueryKeys.graph(sessionId, revision, file, graphDepth, graphLimit),
    enabled: !!project && route?.view === 'graph' && api.isTauri,
    queryFn: async () => validateRevision(await api.getGraph(sessionId, file, {
      depth: graphDepth,
      limit: graphLimit,
    })),
  })

  return {
    file,
    graphKey: graphCacheKey(file, graphDepth, graphLimit),
    fileQuery,
    dimensionQuery,
    graphQuery,
    mockDimension: !api.isTauri && dimension ? MOCK_DIMENSION_FILE_RECORDS[file] : undefined,
    mockGraph: !api.isTauri && route?.view === 'graph' ? MOCK_GRAPH : undefined,
  }
}
