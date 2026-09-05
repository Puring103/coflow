import { useCallback, useEffect, useRef, useState } from 'react'
import * as api from '../api'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import {
  replaceLocalReadPlugins,
  setReadPluginDataApi,
  setReadPluginEnabled,
  setReadPluginSession,
  useReadPluginSettings,
} from '../plugins'
import type { ReadPlugin } from '../plugins/types'
import { errorMessage } from '../wire'

export function useFrontendPlugins(project: ProjectBootstrap | null) {
  const settings = useReadPluginSettings()
  const restored = useRef(false)
  const globalBundles = useRef<api.FrontendPluginBundle[]>([])
  const [globalReady, setGlobalReady] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setReadPluginDataApi({
      getSchema: api.getPluginSchema,
      getRecordsByType: api.getPluginRecordsByType,
    })
  }, [])

  useEffect(() => {
    setReadPluginSession(project?.session_id ?? null)
  }, [project?.session_id])

  useEffect(() => {
    if (!api.isTauri || restored.current) return
    restored.current = true
    api.listFrontendPlugins().then(bundles => {
      globalBundles.current = bundles
      setGlobalReady(true)
    }).catch(cause => setError(`加载插件失败：${errorMessage(cause)}`))
  }, [])

  useEffect(() => {
    if (!api.isTauri || !globalReady) return
    const sessionId = project?.session_id
    const projectBundles = sessionId === undefined
      ? Promise.resolve([])
      : api.listProjectFrontendPlugins(sessionId)
    projectBundles.then(async bundles => {
      const errors = await replaceLocalReadPlugins([...globalBundles.current, ...bundles])
      if (errors.length > 0) setError(`部分插件未加载：${errors.join('; ')}`)
    }).catch(cause => setError(`加载项目插件失败：${errorMessage(cause)}`))
  }, [globalReady, project?.session_id])

  const install = useCallback(async () => {
    if (!project) {
      setError('请先打开项目')
      return
    }
    const manifestPath = await api.pickFrontendPluginManifest()
    if (!manifestPath) return
    setBusy(true)
    setError(null)
    try {
      const bundle = await api.installProjectFrontendPlugin(project.session_id, manifestPath)
      await replaceLocalReadPlugins([
        ...globalBundles.current,
        ...await api.listProjectFrontendPlugins(project.session_id),
      ])
      setReadPluginEnabled(bundle.id, true)
    } catch (cause) {
      setError(`加载插件失败：${errorMessage(cause)}`)
    } finally {
      setBusy(false)
    }
  }, [project])

  const uninstall = useCallback(async (plugin: ReadPlugin) => {
    setError(null)
    try {
      if (plugin.origin === 'project') {
        if (!project) return
        await api.uninstallProjectFrontendPlugin(project.session_id, plugin.id)
        await replaceLocalReadPlugins([
          ...globalBundles.current,
          ...await api.listProjectFrontendPlugins(project.session_id),
        ])
      } else {
        await api.uninstallFrontendPlugin(plugin.id)
        globalBundles.current = globalBundles.current.filter(item => item.id !== plugin.id)
        await replaceLocalReadPlugins(globalBundles.current)
      }
    } catch (cause) {
      setError(`卸载插件失败：${errorMessage(cause)}`)
    }
  }, [project])

  const toggle = useCallback(async (plugin: ReadPlugin, enabled: boolean) => {
    try {
      if (plugin.origin === 'project') {
        if (!project) return
        await api.setProjectFrontendPluginEnabled(project.session_id, plugin.id, enabled)
      }
      setReadPluginEnabled(plugin.id, enabled)
    } catch (cause) {
      setError(`更新插件状态失败：${errorMessage(cause)}`)
    }
  }, [project])

  return { settings, busy, error, install, uninstall, toggle }
}
