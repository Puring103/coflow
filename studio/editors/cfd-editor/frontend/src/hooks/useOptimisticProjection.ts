import { useCallback, type Dispatch, type MutableRefObject, type SetStateAction } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { cloneValue, type FieldPathSegment, type FieldValue } from '../wire'
import { projectFieldValue, projectFieldValueAtRevision } from '../state/fieldProjection'
import { projectGraphRows } from '../state/appSupport'
import type { ProjectGenerationController } from '../state/editorState'

interface OptimisticProjectionOptions {
  generation: ProjectGenerationController
  fileDataCacheRef: MutableRefObject<Record<string, FileRecords>>
  graphCacheRef: MutableRefObject<Record<string, GraphData>>
  setFileDataCache: Dispatch<SetStateAction<Record<string, FileRecords>>>
  setGraphCache: Dispatch<SetStateAction<Record<string, GraphData>>>
}

/** 乐观编辑与条件回滚共同拥有投影写入规则，页面只组装操作入口。 */
export function useOptimisticProjection({
  generation, fileDataCacheRef, graphCacheRef, setFileDataCache, setGraphCache,
}: OptimisticProjectionOptions) {
  const optimisticWriteField = useCallback((
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => {
    let appliedIdentity = generation.currentIdentity()
    let oldValue: FieldValue | undefined
    const optimisticValue = cloneValue(newValue)
    const apply = () => {
      const identity = generation.currentIdentity()
      const current = fileDataCacheRef.current[filePath]
      if (!identity || !current) return { changed: true }
      const projection = projectFieldValueAtRevision(
        current,
        identity.revision,
        coordinate,
        fieldPath,
        optimisticValue,
      )
      if (!projection) return { changed: true }
      if (!projection.changed) {
        if (appliedIdentity?.sessionId === identity.sessionId) appliedIdentity = identity
        return { changed: false, row: projection.row }
      }
      if (!projection.row || !projection.oldValue) return { changed: true }
      appliedIdentity = identity
      oldValue = projection.oldValue
      const projectedCache = { ...fileDataCacheRef.current, [filePath]: projection.records }
      setFileDataCache(projectedCache)
      const projectedGraphs = projectGraphRows(
        graphCacheRef.current,
        current.revision,
        [projection.row],
      )
      setGraphCache(projectedGraphs)
      return { changed: true, row: projection.row }
    }
    const initial = apply()
    return {
      ...initial,
      reapply: () => { apply() },
      rollback: () => {
        if (
          !appliedIdentity
          || !oldValue
          || !generation.isCurrent(appliedIdentity.sessionId, appliedIdentity.revision)
        ) return
        const latest = fileDataCacheRef.current[filePath]
        if (!latest) return
        const stillOptimistic = projectFieldValue(latest, coordinate, fieldPath, optimisticValue)
        if (stillOptimistic.changed) return
        const rollback = projectFieldValue(latest, coordinate, fieldPath, oldValue)
        if (!rollback.changed || !rollback.row) return
        const nextCache = { ...fileDataCacheRef.current, [filePath]: rollback.records }
        setFileDataCache(nextCache)
        const nextGraphs = projectGraphRows(
          graphCacheRef.current,
          latest.revision,
          [rollback.row],
        )
        setGraphCache(nextGraphs)
        appliedIdentity = null
      },
    }
  }, [generation, fileDataCacheRef, graphCacheRef, setFileDataCache, setGraphCache])

  return { optimisticWriteField }
}
