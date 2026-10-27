import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react'
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

export function useProjectSettings(
  generation: ProjectGenerationController,
  setError: Dispatch<SetStateAction<string | null>>,
) {
  const [settings, setSettings] = useState<EditorProjectSettings | null>(null)
  const recordGroupSaveQueue = useRef(Promise.resolve())
  const viewsSaveQueue = useRef(Promise.resolve())
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
    const identity = generation.currentIdentity()
    if (!api.isTauri || !identity) return
    // 局部乐观状态不被服务端的旧整份快照覆盖；同类写入按提交顺序持久化。
    recordGroupSaveQueue.current = recordGroupSaveQueue.current.then(async () => {
      if (generation.currentSession() !== identity.sessionId) return
      await api.setRecordGroups(identity.sessionId, filePath, actualType, groups)
    }).catch(error => {
      if (generation.currentSession() === identity.sessionId) setError(`保存记录分组失败: ${errorMessage(error)}`)
    })
  }, [generation, setError])

  const saveViews = useCallback(async (filePath: string, actualType: string, views: ViewConfig[]) => {
    const identity = generation.currentIdentity()
    if (!api.isTauri) {
      setSettings(current => withViews(current, filePath, actualType, views))
      return
    }
    if (!identity) throw new Error('项目尚未打开')
    // 成功后发布设置；失败保留对话框草稿，避免把未保存的配置显示为已保存。
    const task = viewsSaveQueue.current.then(async () => {
      if (generation.currentSession() !== identity.sessionId) throw new Error('项目已切换')
      const next = await api.setViews(identity.sessionId, filePath, actualType, views)
      if (generation.currentSession() !== identity.sessionId) throw new Error('项目已切换')
      setSettings(current => withViews(current, filePath, actualType, next.views[filePath]?.[actualType] ?? []))
    })
    viewsSaveQueue.current = task.catch(() => {})
    await task
  }, [generation])

  return { settings, setSettings, saveRecordGroups, saveViews, saveViewOrder }
}
