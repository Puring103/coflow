import { useCallback, useRef, useState, type SetStateAction } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { GraphData } from '../bindings/GraphData'

type FileProjection = Record<string, FileRecords>
type GraphProjection = Record<string, GraphData>

function resolveState<T>(action: SetStateAction<T>, current: T): T {
  return typeof action === 'function'
    ? (action as (previous: T) => T)(current)
    : action
}

export function useEditorProjections() {
  const [files, setFilesState] = useState<FileProjection>({})
  const [graphs, setGraphsState] = useState<GraphProjection>({})
  const filesRef = useRef(files)
  const graphsRef = useRef(graphs)

  // 乐观 mutation 在异步回调中必须读到最新投影，因此 state 与 ref 在同一入口原子更新。
  const setFiles = useCallback((action: SetStateAction<FileProjection>) => {
    const next = resolveState(action, filesRef.current)
    filesRef.current = next
    setFilesState(next)
  }, [])
  const setGraphs = useCallback((action: SetStateAction<GraphProjection>) => {
    const next = resolveState(action, graphsRef.current)
    graphsRef.current = next
    setGraphsState(next)
  }, [])
  const reset = useCallback(() => {
    filesRef.current = {}
    graphsRef.current = {}
    setFilesState({})
    setGraphsState({})
  }, [])

  return { files, graphs, filesRef, graphsRef, setFiles, setGraphs, reset }
}
