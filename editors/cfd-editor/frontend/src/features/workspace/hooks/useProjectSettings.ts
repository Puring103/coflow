import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from 'react'
import type { EditorProjectSettings } from '../../../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../../../bindings/EditorRecordGroup'
import type { ViewConfig } from '../../../bindings/ViewConfig'
import type { ProjectGenerationController } from '../state/editorState'
import { errorMessage } from '../../../wire'
import * as api from '../../../api'

export function emptyProjectSettings(): EditorProjectSettings {
  return {
    graph_positions: {},
    graph_compact_modes: {},
    views: {},
    view_order: {},
    short_name_fields: {},
    default_table_column_widths: {},
    table_row_heights: {},
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

function persistLatestSettings(
  sequenceRef: MutableRefObject<number>,
  generation: ProjectGenerationController,
  setSettings: Dispatch<SetStateAction<EditorProjectSettings | null>>,
  setError: Dispatch<SetStateAction<string | null>>,
  errorPrefix: string,
  request: (sessionId: number) => Promise<EditorProjectSettings>,
): void {
  const sequence = ++sequenceRef.current
  const identity = generation.currentIdentity()
  if (!api.isTauri || !identity) return
  request(identity.sessionId)
    .then(next => {
      if (generation.currentSession() === identity.sessionId && sequenceRef.current === sequence) {
        setSettings(next)
      }
    })
    .catch(error => {
      if (generation.currentSession() === identity.sessionId) {
        setError(`${errorPrefix}: ${errorMessage(error)}`)
      }
    })
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
    setSettings(current => withRecordGroups(current, filePath, actualType, groups))
    persistLatestSettings(
      recordGroupSaveSequence, generation, setSettings, setError, '保存记录分组失败',
      sessionId => api.setRecordGroups(sessionId, filePath, actualType, groups),
    )
  }, [generation, setError])

  const saveViews = useCallback(async (filePath: string, actualType: string, views: ViewConfig[]) => {
    const sequence = ++viewsSaveSequence.current
    const identity = generation.currentIdentity()
    if (!api.isTauri) {
      setSettings(current => withViews(current, filePath, actualType, views))
      return
    }
    if (!identity) throw new Error('项目尚未打开')
    // 成功后发布设置；失败保留对话框草稿，避免把未保存的配置显示为已保存。
    const next = await api.setViews(identity.sessionId, filePath, actualType, views)
    if (generation.currentSession() !== identity.sessionId) throw new Error('项目已切换')
    if (viewsSaveSequence.current === sequence) setSettings(next)
  }, [generation])

  return { settings, setSettings, saveRecordGroups, saveViews, saveViewOrder }
}
