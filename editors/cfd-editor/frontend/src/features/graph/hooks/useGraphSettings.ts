import { useCallback, useRef, type Dispatch, type SetStateAction } from 'react'
import * as api from '../../../api'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'
import { errorMessage } from '../../../wire'
import { emptyProjectSettings } from '../../workspace/hooks/useProjectSettings'
import { committed, failed, superseded, type MutationResult, type GraphPositions, MutationHistoryController, ProjectGenerationController } from '../../workspace/state/editorState'

// 图谱展示设置与布局历史共用同一代次校验和串行提交边界。
export function useGraphSettings(
  projectSettings: EditorProjectSettings | null,
  setProjectSettings: Dispatch<SetStateAction<EditorProjectSettings | null>>,
  generation: ProjectGenerationController,
  history: MutationHistoryController,
  setErrorMsg: Dispatch<SetStateAction<string | null>>,
) {
  const graphSettingsRef = useRef(projectSettings)
  graphSettingsRef.current = projectSettings

  const applyGraphPositions = useCallback(async (viewKey: string, positions: GraphPositions): Promise<MutationResult<void>> => {
    const identity = generation.currentIdentity()
    if (!identity) return superseded()
    try {
      if (api.isTauri) await api.setGraphPositions(identity.sessionId, viewKey, positions)
      if (generation.currentIdentity()?.sessionId !== identity.sessionId) return superseded()
      const base = graphSettingsRef.current ?? emptyProjectSettings()
      graphSettingsRef.current = { ...base, graph_positions: { ...base.graph_positions, [viewKey]: positions } }
      setProjectSettings(current => {
        const base = current ?? emptyProjectSettings()
        return { ...base, graph_positions: { ...base.graph_positions, [viewKey]: positions } }
      })
      return committed(undefined)
    } catch (error) {
      setErrorMsg(`保存图节点位置失败：${errorMessage(error)}`)
      return failed()
    }
  }, [generation, setProjectSettings])

  const saveGraphPositions = useCallback((viewKey: string, positions: GraphPositions, recordHistory: boolean) => {
    const identity = generation.currentIdentity()
    const epoch = history.currentEpoch()
    return history.serialize(async () => {
      if (!identity || generation.currentIdentity()?.sessionId !== identity.sessionId || history.currentEpoch() !== epoch) return
      const oldPositions = (graphSettingsRef.current?.graph_positions[viewKey] ?? {}) as GraphPositions
      const next = structuredClone(positions)
      const result = await applyGraphPositions(viewKey, next)
      if (result.status === 'failed') throw new Error('保存图节点位置失败')
      if (result.status === 'committed' && recordHistory) {
        history.record({ kind: 'graph-layout', viewKey,
          revision: generation.currentIdentity()!.revision,
          oldPositions: structuredClone(oldPositions), newPositions: next })
      }
    })
  }, [generation, history, applyGraphPositions])

  // 图视图缩略/完整模式是纯展示偏好，直接落盘，不进入撤销历史。
  const saveGraphCompactMode = useCallback(async (viewKey: string, compact: boolean) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    // 先乐观更新本地设置，按钮切换即时生效。
    const base = graphSettingsRef.current ?? emptyProjectSettings()
    graphSettingsRef.current = {
      ...base,
      graph_compact_modes: { ...base.graph_compact_modes, [viewKey]: compact },
    }
    setProjectSettings(current => {
      const base = current ?? emptyProjectSettings()
      return { ...base, graph_compact_modes: { ...base.graph_compact_modes, [viewKey]: compact } }
    })
    if (!api.isTauri) return
    try {
      await api.setGraphCompactMode(identity.sessionId, viewKey, compact)
    } catch (error) {
      if (generation.currentSession() === identity.sessionId) {
        setErrorMsg(`保存图视图模式失败：${errorMessage(error)}`)
      }
    }
  }, [generation, setProjectSettings])

  return { applyGraphPositions, saveGraphPositions, saveGraphCompactMode }
}
