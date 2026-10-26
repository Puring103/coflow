import { useCallback, useState, type Dispatch, type SetStateAction } from 'react'
import * as api from '../../../api'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'
import { errorMessage } from '../../../wire'
import { emptyProjectSettings } from '../../workspace/hooks/useProjectSettings'
import { ProjectGenerationController } from '../../workspace/state/editorState'
import { EditorLookupController } from '../../workspace/state/editorLookups'

// 缩略名保存成功后统一失效引用缓存，所有记录展示使用同一修订号。
export function useShortNameSettings(
  projectSettings: EditorProjectSettings | null,
  setProjectSettings: Dispatch<SetStateAction<EditorProjectSettings | null>>,
  generation: ProjectGenerationController,
  lookups: EditorLookupController,
  setErrorMsg: Dispatch<SetStateAction<string | null>>,
) {
  const [shortNameSaving, setShortNameSaving] = useState(false)
  const [shortNameRevision, setShortNameRevision] = useState(0)
  const saveShortNameField = useCallback(async (actualType: string, field: string | null) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    setShortNameSaving(true)
    try {
      const fields = api.isTauri
        ? (await api.setShortNameField(identity.sessionId, actualType, field)).short_name_fields
        : { ...projectSettings?.short_name_fields, [actualType]: field ?? undefined }
      if (generation.currentSession() !== identity.sessionId) return
      // 设置保存成功后刷新引用缓存，所有展示位置一起切换到新字段。
      lookups.invalidateRefTargets()
      setProjectSettings(current => ({ ...(current ?? emptyProjectSettings()), short_name_fields: fields }))
      setShortNameRevision(revision => revision + 1)
    } catch (error) {
      if (generation.currentSession() === identity.sessionId) setErrorMsg(`保存缩略名失败: ${errorMessage(error)}`)
    } finally {
      setShortNameSaving(false)
    }
  }, [generation, lookups, projectSettings?.short_name_fields, setProjectSettings])
  return { shortNameSaving, shortNameRevision, saveShortNameField }
}
