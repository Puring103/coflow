import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { ViewConfig } from '../bindings/ViewConfig'
import type { ProjectGenerationController } from '../state/editorState'
import { errorMessage } from '../wire'
import * as api from '../api'

export function emptyProjectSettings(): EditorProjectSettings {
  return {
    views: {},
    view_order: {},
    short_name_fields: {},
    default_table_column_widths: {},
    record_groups: {},
    workspace: { tabs: [], active_tab_id: null },
  }
}

function withRecordGroups(
  settings: EditorProjectSettings | null,
  filePath: string,
  actualType: string,
  groups: EditorRecordGroup[],
): EditorProjectSettings {
  const base = settings ?? emptyProjectSettings()
  return {
    ...base,
    record_groups: {
      ...base.record_groups,
      [filePath]: {
        ...(base.record_groups?.[filePath] ?? {}),
        [actualType]: groups,
      },
    },
  }
}

function withViews(
  settings: EditorProjectSettings | null,
  filePath: string,
  actualType: string,
  views: ViewConfig[],
): EditorProjectSettings {
  const base = settings ?? emptyProjectSettings()
  return {
    ...base,
    views: {
      ...base.views,
      [filePath]: {
        ...(base.views?.[filePath] ?? {}),
        [actualType]: views,
      },
    },
  }
}

export function useProjectSettings(
  generation: ProjectGenerationController,
  setError: Dispatch<SetStateAction<string | null>>,
) {
  const [settings, setSettings] = useState<EditorProjectSettings | null>(null)
  const recordGroupSaveSequence = useRef(0)
  const viewsSaveSequence = useRef(0)
  const orderSaveQueue = useRef(Promise.resolve())

  const saveViewOrder = useCallback((filePath: string, actualType: string, order: string[]) => {
    setSettings(current => {
      const base = current ?? emptyProjectSettings()
      return { ...base, view_order: {
        ...base.view_order,
        [filePath]: { ...base.view_order[filePath], [actualType]: order },
      } }
    })
    if (!api.isTauri) return
    const identity = generation.currentIdentity()
    if (!identity) return
    // 串行保存连续拖动的结果，避免较早的请求覆盖最终顺序。
    orderSaveQueue.current = orderSaveQueue.current.then(async () => {
      if (generation.currentSession() !== identity.sessionId) return
      await api.setViewOrder(identity.sessionId, filePath, actualType, order)
    }).catch(error => {
      if (generation.currentSession() === identity.sessionId) {
        setError(`保存视图顺序失败: ${errorMessage(error)}`)
      }
    })
  }, [generation, setError])

  const saveRecordGroups = useCallback((
    filePath: string,
    actualType: string,
    groups: EditorRecordGroup[],
  ) => {
    const sequence = ++recordGroupSaveSequence.current
    setSettings(current => withRecordGroups(current, filePath, actualType, groups))
    if (!api.isTauri) return
    const identity = generation.currentIdentity()
    if (!identity) return
    api.setRecordGroups(identity.sessionId, filePath, actualType, groups)
      .then(next => {
        if (generation.currentSession() === identity.sessionId
          && recordGroupSaveSequence.current === sequence) {
          setSettings(next)
        }
      })
      .catch(error => {
        if (generation.currentSession() === identity.sessionId) {
          setError(`保存记录分组失败: ${errorMessage(error)}`)
        }
      })
  }, [generation, setError])

  const saveViews = useCallback((
    filePath: string,
    actualType: string,
    views: ViewConfig[],
  ) => {
    const sequence = ++viewsSaveSequence.current
    setSettings(current => withViews(current, filePath, actualType, views))
    if (!api.isTauri) return
    const identity = generation.currentIdentity()
    if (!identity) return
    api.setViews(identity.sessionId, filePath, actualType, views)
      .then(next => {
        if (generation.currentSession() === identity.sessionId
          && viewsSaveSequence.current === sequence) {
          setSettings(next)
        }
      })
      .catch(error => {
        if (generation.currentSession() === identity.sessionId) {
          setError(`保存视图失败: ${errorMessage(error)}`)
        }
      })
  }, [generation, setError])

  return { settings, setSettings, saveRecordGroups, saveViews, saveViewOrder }
}
