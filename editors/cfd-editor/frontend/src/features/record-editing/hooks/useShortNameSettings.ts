import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import * as api from '../../../api'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'
import { errorMessage } from '../../../wire'
import { emptyProjectSettings } from '../../workspace/hooks/useProjectSettings'
import { ProjectGenerationController } from '../../workspace/state/editorState'
import { EditorLookupController } from '../../workspace/state/editorLookups'

// 缩略名变更后统一失效引用缓存，所有记录展示使用同一修订号。
export function useShortNameSettings(
  setProjectSettings: Dispatch<SetStateAction<EditorProjectSettings | null>>,
  generation: ProjectGenerationController,
  lookups: EditorLookupController,
  setErrorMsg: Dispatch<SetStateAction<string | null>>,
) {
  const [shortNameSaving, setShortNameSaving] = useState(false)
  const [shortNameRevision, setShortNameRevision] = useState(0)
  const saveQueue = useRef(Promise.resolve())
  const refreshShortNames = useCallback(() => {
    lookups.invalidateRefTargets()
    setShortNameRevision(revision => revision + 1)
  }, [lookups])
  const saveShortNameField = useCallback(async (actualType: string, field: string | null) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    setShortNameSaving(true)
    try {
      if (api.isTauri) {
        const task = saveQueue.current.then(async () => {
          if (generation.currentSession() === identity.sessionId) await api.setShortNameField(identity.sessionId, actualType, field)
        })
        saveQueue.current = task.catch(() => {})
        await task
      }
      if (generation.currentSession() !== identity.sessionId) return
      // 设置保存成功后刷新引用缓存，所有展示位置一起切换到新字段。
      refreshShortNames()
      setProjectSettings(current => {
        const base = current ?? emptyProjectSettings()
        const fields = { ...base.short_name_fields }
        if (field === null) delete fields[actualType]
        else fields[actualType] = field
        return { ...base, short_name_fields: fields }
      })
    } catch (error) {
      if (generation.currentSession() === identity.sessionId) setErrorMsg(`保存缩略名失败: ${errorMessage(error)}`)
    } finally {
      setShortNameSaving(false)
    }
  }, [generation, refreshShortNames, setProjectSettings, setErrorMsg])
  return { shortNameSaving, shortNameRevision, saveShortNameField, refreshShortNames }
}
