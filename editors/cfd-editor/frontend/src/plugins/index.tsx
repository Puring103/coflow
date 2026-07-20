import { useEffect, useMemo, useRef, useSyncExternalStore, type ReactNode } from 'react'
import type { FrontendPluginBundle } from '../api'
import type {
  ExtensionActivate,
  ExtensionHost,
  FieldRenderer,
  ReadPlugin,
  ReadRenderContext,
} from './types'

const ENABLED_STORAGE_KEY = 'cfd-editor-enabled-read-plugins'
const plugins: ReadPlugin[] = []
const scriptUrls = new Map<string, string>()
const listeners = new Set<() => void>()
let enabledIds = storedPluginIds()
let revision = 0

function storedPluginIds(): Set<string> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(ENABLED_STORAGE_KEY) ?? '[]')
    return Array.isArray(parsed) && parsed.every(item => typeof item === 'string') ? new Set(parsed) : new Set()
  } catch {
    return new Set()
  }
}

function notify() {
  revision += 1
  listeners.forEach(listener => listener())
}

function matchesTarget(renderer: FieldRenderer, context: ReadRenderContext): boolean {
  const target = renderer.target
  return target.kind === 'field-value'
    && target.type === context.type
    && target.surfaces.includes(context.surface)
}

function pluginHost(renderers: FieldRenderer[]): ExtensionHost {
  return {
    apiVersion: 1,
    renderers: {
      register(renderer) {
        if (!renderer.id || !renderer.target || typeof renderer.mount !== 'function') {
          throw new Error('renderer requires id, target, and mount')
        }
        const target = renderer.target
        if (target.kind !== 'field-value' || !target.type.trim() || !Array.isArray(target.surfaces)
          || target.surfaces.length === 0
          || target.surfaces.some(surface => surface !== 'table-cell' && surface !== 'record-foldout-header')) {
          throw new Error('renderer target requires a type and supported surfaces')
        }
        if (renderers.some(item => item.id === renderer.id)) {
          throw new Error(`duplicate renderer id \`${renderer.id}\``)
        }
        renderers.push(renderer)
        return () => {
          const index = renderers.indexOf(renderer)
          if (index >= 0) renderers.splice(index, 1)
        }
      },
    },
  }
}

export async function loadLocalReadPlugin(bundle: FrontendPluginBundle): Promise<void> {
  unloadLocalReadPlugin(bundle.id)
  const url = URL.createObjectURL(new Blob([bundle.source], { type: 'text/javascript' }))
  try {
    const module = await import(/* @vite-ignore */ url) as { default?: ExtensionActivate }
    if (typeof module.default !== 'function') {
      throw new Error('plugin must export default function activate(host)')
    }
    const renderers: FieldRenderer[] = []
    const definition = await module.default(pluginHost(renderers))
    if (renderers.length === 0) throw new Error('plugin did not register a renderer')
    plugins.unshift({
      id: bundle.id,
      name: bundle.name,
      description: bundle.description,
      version: bundle.version,
      renderers,
      dispose: definition?.dispose,
      origin: 'local',
      manifestPath: bundle.manifest_path,
    })
    scriptUrls.set(bundle.id, url)
    enabledIds.add(bundle.id)
    localStorage.setItem(ENABLED_STORAGE_KEY, JSON.stringify([...enabledIds]))
    notify()
  } catch (error) {
    URL.revokeObjectURL(url)
    throw error
  }
}

export function unloadLocalReadPlugin(id: string) {
  const index = plugins.findIndex(plugin => plugin.id === id)
  if (index < 0) return
  const [plugin] = plugins.splice(index, 1)
  plugin.dispose?.()
  enabledIds.delete(id)
  const url = scriptUrls.get(id)
  if (url) URL.revokeObjectURL(url)
  scriptUrls.delete(id)
  localStorage.setItem(ENABLED_STORAGE_KEY, JSON.stringify([...enabledIds]))
  notify()
}

export async function restoreLocalReadPlugins(bundles: FrontendPluginBundle[]): Promise<string[]> {
  const errors: string[] = []
  for (const bundle of bundles) {
    try {
      await loadLocalReadPlugin(bundle)
    } catch (error) {
      errors.push(`${bundle.name}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }
  return errors
}

export function setReadPluginEnabled(id: string, enabled: boolean) {
  if (enabled) enabledIds.add(id)
  else enabledIds.delete(id)
  localStorage.setItem(ENABLED_STORAGE_KEY, JSON.stringify([...enabledIds]))
  notify()
}

export function useReadPlugins(): readonly ReadPlugin[] {
  const currentRevision = useSyncExternalStore(
    listener => { listeners.add(listener); return () => listeners.delete(listener) },
    () => revision,
    () => 0,
  )
  return useMemo(() => plugins.filter(plugin => enabledIds.has(plugin.id)), [currentRevision])
}

export function useReadPluginSettings(): ReadonlyArray<ReadPlugin & { enabled: boolean }> {
  useSyncExternalStore(
    listener => { listeners.add(listener); return () => listeners.delete(listener) },
    () => revision,
    () => 0,
  )
  return plugins.map(plugin => ({ ...plugin, enabled: enabledIds.has(plugin.id) }))
}

export function useFieldRenderer(context: ReadRenderContext): FieldRenderer | undefined {
  const active = useReadPlugins()
  return active.flatMap(plugin => plugin.renderers).find(renderer => matchesTarget(renderer, context))
}

export function PluginRendererMount({ renderer, context, fallback }: {
  renderer: FieldRenderer | undefined
  context: ReadRenderContext
  fallback: ReactNode
}) {
  const elementRef = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    const element = elementRef.current
    if (!renderer || !element) return
    const controller = new AbortController()
    const cleanup = renderer.mount(context, {
      element,
      signal: controller.signal,
      replace(content) { element.replaceChildren(content) },
    })
    return () => {
      controller.abort()
      cleanup?.()
      element.replaceChildren()
    }
  }, [context, renderer])
  return renderer ? <span className="dc-plugin-value" ref={elementRef} /> : <>{fallback}</>
}
