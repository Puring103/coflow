import { recordIncident } from '../diagnostics'
import { flushSourceChanges } from '../state/sourceAutosave'
import { flushDataEdits } from '../state/editSession'
import { getVersion } from '@tauri-apps/api/app'
import { relaunch } from '@tauri-apps/plugin-process'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { useCallback, useEffect, useRef, useState } from 'react'
import { isTauri } from '../api'

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up-to-date'
  | 'available'
  | 'downloading'
  | 'restarting'
  | 'error'

export interface UpdaterState {
  phase: UpdatePhase
  currentVersion: string
  availableVersion?: string
  progress?: number
  error?: string
}

const INITIAL_STATE: UpdaterState = {
  phase: 'idle',
  currentVersion: '',
}

export function useUpdater() {
  const [state, setState] = useState<UpdaterState>(INITIAL_STATE)
  const updateRef = useRef<Update | null>(null)
  const autoChecked = useRef(false)

  const checkForUpdate = useCallback(async (showError: boolean) => {
    if (!isTauri) return
    setState(current => ({ ...current, phase: 'checking', error: undefined }))
    try {
      const update = await check({ timeout: 15_000 })
      if (updateRef.current && updateRef.current !== update) {
        await updateRef.current.close().catch(() => undefined)
      }
      updateRef.current = update
      setState(current => update
        ? {
            ...current,
            phase: 'available',
            currentVersion: update.currentVersion,
            availableVersion: update.version,
          }
        : { ...current, phase: 'up-to-date', availableVersion: undefined })
    } catch (error) {
      recordIncident("updater", error)
      setState(current => showError
        ? { ...current, phase: 'error', error: updateErrorMessage(error) }
        : { ...current, phase: 'idle' })
    }
  }, [])

  useEffect(() => {
    if (!isTauri || autoChecked.current) return
    autoChecked.current = true
    void getVersion()
      .then(version => setState(current => ({ ...current, currentVersion: version })))
      .catch(() => undefined)
    void checkForUpdate(false)
  }, [checkForUpdate])

  const activate = useCallback(async () => {
    if (state.phase === 'checking' || state.phase === 'downloading' || state.phase === 'restarting') {
      return
    }
    if (state.phase !== 'available' || !updateRef.current) {
      await checkForUpdate(true)
      return
    }

    const update = updateRef.current
    let downloaded = 0
    let total: number | undefined
    setState(current => ({ ...current, phase: 'downloading', progress: 0, error: undefined }))
    try {
      if (!await flushDataEdits()) throw new Error("数据编辑尚未成功保存，请处理后重试更新")
      if (!await flushSourceChanges()) throw new Error("有未保存内容或项目尚未恢复，请处理后再安装更新")
      await update.download(event => {
        if (event.event === 'Started') {
          total = event.data.contentLength
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
          const progress = total && total > 0
            ? Math.min(100, Math.round(downloaded * 100 / total))
            : undefined
          setState(current => ({ ...current, progress }))
        }
      })
      if (!await flushDataEdits()) throw new Error("数据编辑尚未成功保存，请处理后重试更新")
      if (!await flushSourceChanges()) throw new Error('更新已下载，但有未保存内容；请保存后重试安装')
      await update.install()
      setState(current => ({ ...current, phase: 'restarting', progress: 100 }))
      await relaunch()
    } catch (error) {
      recordIncident("updater", error)
      setState(current => ({
        ...current,
        phase: 'error',
        error: updateErrorMessage(error),
      }))
    }
  }, [checkForUpdate, state.phase])

  return { state, activate }
}

function updateErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
