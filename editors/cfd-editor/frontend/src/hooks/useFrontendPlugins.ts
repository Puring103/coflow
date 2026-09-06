import { useCallback, useEffect, useRef, useState } from 'react'
import * as api from '../api'
import { builtInPlugins } from '../built-in-plugins'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import {
  replaceFrontendPlugins,
  setFrontendPluginEnabled,
  setPluginProjectDefaults,
  unloadFrontendPlugin,
  usePluginSettings,
  type PluginMetadata,
} from '../plugins'
import { errorMessage } from '../wire'

export function useFrontendPlugins(project: ProjectBootstrap | null) {
  const settings = usePluginSettings()
  const restored = useRef(false)
  const loadSequence = useRef(0)
  const globalBundles = useRef<api.FrontendPluginBundle[]>([])
  const globalErrors = useRef<string[]>([])
  const [globalReady, setGlobalReady] = useState(!api.isTauri)
  const [ready, setReady] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!api.isTauri || restored.current) return
    restored.current = true
    api.listFrontendPlugins().then(state => {
      globalBundles.current = state.plugins
      globalErrors.current = state.errors
      if (state.errors.length > 0) setError(`部分插件未加载：${state.errors.join('; ')}`)
      setGlobalReady(true)
    }).catch(cause => {
      globalErrors.current = [errorMessage(cause)]
      setError(`加载插件失败：${globalErrors.current[0]}`)
      setGlobalReady(true)
    })
  }, [])

  useEffect(() => {
    if (!globalReady) return
    const sequence = ++loadSequence.current
    setReady(false)
    const sessionId = project?.session_id
    const projectState = sessionId === undefined
      ? Promise.resolve<api.FrontendPluginProjectState>({
          plugins: [],
          errors: [],
          defaults: { views: {}, presentations: {} },
        })
      : api.listProjectFrontendPlugins(sessionId)
    projectState.then(async state => {
      if (loadSequence.current !== sequence) return
      setPluginProjectDefaults(state.defaults)
      const activationErrors = await replaceFrontendPlugins(builtInPlugins, [...globalBundles.current, ...state.plugins])
      if (loadSequence.current !== sequence) return
      const errors = [...globalErrors.current, ...state.errors, ...activationErrors]
      setError(errors.length > 0 ? `部分插件未加载：${errors.join('; ')}` : null)
      setReady(true)
    }).catch(cause => {
      if (loadSequence.current === sequence) {
        setError(`加载项目插件失败：${errorMessage(cause)}`)
        setReady(true)
      }
    })
  }, [globalReady, project?.session_id])

  const reloadPlugins = useCallback(async () => {
    setReady(false)
    try {
      const state = project
        ? await api.listProjectFrontendPlugins(project.session_id)
        : {
            plugins: [],
            errors: [],
            defaults: { views: {}, presentations: {} },
          }
      setPluginProjectDefaults(state.defaults)
      const activationErrors = await replaceFrontendPlugins(builtInPlugins, [...globalBundles.current, ...state.plugins])
      const errors = [...globalErrors.current, ...state.errors, ...activationErrors]
      setError(errors.length > 0 ? `部分插件未加载：${errors.join('; ')}` : null)
    } finally {
      setReady(true)
    }
  }, [project])

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
      await reloadPlugins()
      setFrontendPluginEnabled(bundle.id, true)
    } catch (cause) {
      setError(`加载插件失败：${errorMessage(cause)}`)
    } finally {
      setBusy(false)
    }
  }, [project, reloadPlugins])

  const uninstall = useCallback(async (plugin: PluginMetadata) => {
    setError(null)
    try {
      if (plugin.origin === 'project') {
        if (!project) return
        await api.uninstallProjectFrontendPlugin(project.session_id, plugin.id)
        unloadFrontendPlugin(plugin.id)
        await reloadPlugins()
      } else {
        await api.uninstallFrontendPlugin(plugin.id)
        globalBundles.current = globalBundles.current.filter(item => item.id !== plugin.id)
        unloadFrontendPlugin(plugin.id)
        await reloadPlugins()
      }
    } catch (cause) {
      setError(`卸载插件失败：${errorMessage(cause)}`)
    }
  }, [project, reloadPlugins])

  const toggle = useCallback(async (plugin: PluginMetadata, enabled: boolean) => {
    try {
      if (plugin.origin === 'project') {
        if (!project) return
        await api.setProjectFrontendPluginEnabled(project.session_id, plugin.id, enabled)
      }
      setFrontendPluginEnabled(plugin.id, enabled)
      await reloadPlugins()
    } catch (cause) {
      setError(`更新插件状态失败：${errorMessage(cause)}`)
    }
  }, [project, reloadPlugins])

  return { settings, ready, busy, error, install, uninstall, toggle }
}
