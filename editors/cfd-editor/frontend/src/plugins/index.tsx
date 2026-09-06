import { createElement, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import type { FrontendPluginBundle } from '../api'
import type { RecordRow } from '../bindings/RecordRow'
import { cloneValue, sameCoordinate, type DictKey, type FieldPathSegment, type FieldValue } from '../wire'
import type {
  EditorPluginActivate,
  EditorPluginHost,
  PluginActiveContext,
  PluginCommandContribution,
  PluginCommandContext,
  PluginContributionBase,
  PluginDataApi,
  PluginDataBridge,
  PluginEventMap,
  PluginKeybindingContribution,
  PluginMetadata,
  PluginMountedPresentation,
  PluginMutationRequest,
  PluginOutlet,
  PluginPageContext,
  PluginPageContribution,
  PluginPresentationContext,
  PluginPresentationContribution,
  PluginPresentationSlot,
  PluginProjectDefaults,
  PluginRecordData,
  PluginSearchHit,
  PluginSidebarContext,
  PluginSidebarContribution,
  PluginSummaryPresentation,
  PluginUiBridge,
  PluginUiInstance,
  PluginViewContext,
  PluginViewContribution,
} from './types'

export * from './types'

const DISABLED_STORAGE_KEY = 'cfd-editor-disabled-plugins'

export type RegisteredContribution<T> = T & {
  pluginId: string
  key: string
  order: number
}

type RegisteredKeybinding = RegisteredContribution<
  PluginKeybindingContribution & { id: string; accelerator: string }
>

interface PluginRuntime extends PluginMetadata {
  pages: RegisteredContribution<PluginPageContribution>[]
  views: RegisteredContribution<PluginViewContribution>[]
  sidebars: RegisteredContribution<PluginSidebarContribution>[]
  presentations: RegisteredContribution<PluginPresentationContribution>[]
  commands: RegisteredContribution<PluginCommandContribution>[]
  keybindings: RegisteredKeybinding[]
  nextKeybindingId: number
  eventDisposers: Array<() => void>
  dispose?: () => void
}

export interface PluginRegistrySnapshot {
  revision: number
  plugins: readonly PluginMetadata[]
  pages: readonly RegisteredContribution<PluginPageContribution>[]
  views: readonly RegisteredContribution<PluginViewContribution>[]
  sidebars: readonly RegisteredContribution<PluginSidebarContribution>[]
  presentations: readonly RegisteredContribution<PluginPresentationContribution>[]
  commands: readonly RegisteredContribution<PluginCommandContribution>[]
  keybindings: readonly RegisteredKeybinding[]
  defaults: PluginProjectDefaults
}

export interface BuiltInPluginDefinition {
  id: string
  name: string
  description: string
  version: string
  activate: EditorPluginActivate
}

const EMPTY_DEFAULTS: PluginProjectDefaults = { views: {}, presentations: {} }
const listeners = new Set<() => void>()
const runtimes: PluginRuntime[] = []
const scriptUrls = new Map<string, string>()
const failedContributions = new Set<string>()
let registryRevision = 0
let dataBridge: PluginDataBridge | null = null
let uiBridge: PluginUiBridge | null = null
let projectDefaults: PluginProjectDefaults = EMPTY_DEFAULTS
let disabledGlobalIds = storedPluginIds()
let replacementQueue: Promise<void> = Promise.resolve()
let mutationQueue: Promise<void> = Promise.resolve()

interface EventListenerEntry<K extends keyof PluginEventMap> {
  pluginId: string
  listener: (payload: PluginEventMap[K]) => void | Promise<void>
}

const eventListeners: { [K in keyof PluginEventMap]: Set<EventListenerEntry<K>> } = {
  project: new Set(),
  data: new Set(),
  selection: new Set(),
  surface: new Set(),
}

let snapshot: PluginRegistrySnapshot = buildSnapshot()

function storedPluginIds(): Set<string> {
  try {
    const value = JSON.parse(localStorage.getItem(DISABLED_STORAGE_KEY) ?? '[]')
    return new Set(Array.isArray(value) ? value.filter(item => typeof item === 'string') : [])
  } catch {
    return new Set()
  }
}

function isEnabled(runtime: PluginRuntime): boolean {
  if (runtime.origin === 'built-in') return true
  return runtime.origin === 'project' ? runtime.enabled : !disabledGlobalIds.has(runtime.id)
}

function active<T extends { key: string }>(items: readonly T[]): T[] {
  return items.filter(item => !failedContributions.has(item.key))
}

function buildSnapshot(): PluginRegistrySnapshot {
  const enabled = runtimes.filter(isEnabled)
  return {
    revision: registryRevision,
    plugins: runtimes.map(runtime => ({
      id: runtime.id,
      name: runtime.name,
      description: runtime.description,
      version: runtime.version,
      origin: runtime.origin,
      manifestPath: runtime.manifestPath,
      enabled: isEnabled(runtime),
    })),
    pages: active(enabled.flatMap(runtime => runtime.pages)),
    views: active(enabled.flatMap(runtime => runtime.views)),
    sidebars: active(enabled.flatMap(runtime => runtime.sidebars)),
    presentations: active(enabled.flatMap(runtime => runtime.presentations)),
    commands: active(enabled.flatMap(runtime => runtime.commands)),
    keybindings: active(enabled.flatMap(runtime => runtime.keybindings)),
    defaults: projectDefaults,
  }
}

function notify(): void {
  registryRevision += 1
  snapshot = buildSnapshot()
  listeners.forEach(listener => listener())
}

export function setPluginDataBridge(bridge: PluginDataBridge | null): void {
  dataBridge = bridge
}

export function currentPluginIdentity() {
  return dataBridge?.currentIdentity() ?? null
}

export function setPluginUiBridge(bridge: PluginUiBridge | null): void {
  uiBridge = bridge
}

export function setPluginProjectDefaults(defaults: PluginProjectDefaults | null): void {
  projectDefaults = defaults ?? EMPTY_DEFAULTS
  notify()
}

export function pluginRegistrySnapshot(): PluginRegistrySnapshot {
  return snapshot
}

export function subscribePluginRegistry(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function usePluginRegistry(): PluginRegistrySnapshot {
  return useSyncExternalStore(subscribePluginRegistry, pluginRegistrySnapshot, pluginRegistrySnapshot)
}

export function usePluginSettings(): readonly PluginMetadata[] {
  return usePluginRegistry().plugins.filter(plugin => plugin.origin !== 'built-in')
}

function contributionKey(pluginId: string, contributionId: string): string {
  return `${pluginId}/${contributionId}`
}

function validateId(id: string, label: string): void {
  if (!id.trim() || !/^[A-Za-z0-9_-]+$/.test(id)) {
    throw new Error(`${label} id 只能包含 ASCII 字母、数字、连字符和下划线`)
  }
}

function validateBase(contribution: PluginContributionBase, label: string): void {
  validateId(contribution.id, label)
  if (!contribution.title.trim()) throw new Error(`${label}需要非空标题`)
}

function validateTargets(types: unknown): asserts types is string[] {
  if (!Array.isArray(types) || types.length === 0
    || types.some(type => typeof type !== 'string' || !type.trim() || type.includes('*'))) {
    throw new Error('类型目标必须是非空的明确类型列表')
  }
}

function runtimeContributionIds(runtime: PluginRuntime): Set<string> {
  return new Set([
    ...runtime.pages,
    ...runtime.views,
    ...runtime.sidebars,
    ...runtime.presentations,
    ...runtime.commands,
  ].map(item => item.id))
}

function registerContribution<T extends { id: string }>(
  runtime: PluginRuntime,
  contribution: T,
  target: RegisteredContribution<T>[],
): () => void {
  if (runtimeContributionIds(runtime).has(contribution.id)) {
    throw new Error(`插件贡献 id \`${contribution.id}\` 重复`)
  }
  const item = {
    ...contribution,
    pluginId: runtime.id,
    key: contributionKey(runtime.id, contribution.id),
    order: target.length,
  }
  target.push(item)
  // 激活完成后仍允许插件按需注册贡献，注册表必须立即发布新快照。
  if (runtimes.includes(runtime)) notify()
  return () => {
    const index = target.indexOf(item)
    if (index >= 0) {
      target.splice(index, 1)
      failedContributions.delete(item.key)
      notify()
    }
  }
}

function createDataApi(): PluginDataApi {
  const capture = () => {
    const bridge = requireDataBridge()
    const identity = bridge.currentIdentity()
    if (!identity) throw new Error('当前没有打开的项目')
    return { bridge, identity }
  }
  const ensureCurrent = (identity: { sessionId: number; revision: number }) => {
    const current = requireDataBridge().currentIdentity()
    if (!current || current.sessionId !== identity.sessionId || current.revision !== identity.revision) {
      throw new Error('插件查询的数据修订已过期')
    }
  }
  return {
    current: () => dataBridge?.currentIdentity() ?? null,
    async getSchema() {
      const { bridge, identity } = capture()
      const data = await bridge.getSchema(identity.sessionId)
      ensureCurrent(identity)
      return { ...identity, data: structuredClone(data) }
    },
    async getRecordsByType(typeName, options) {
      validateExactType(typeName)
      const { bridge, identity } = capture()
      const rows = await bridge.getRecordsByType(identity.sessionId, typeName)
      ensureCurrent(identity)
      return {
        ...identity,
        data: rows.map(row => projectRecord(row, options?.includeFieldValues !== false)),
      }
    },
    async getRecord(filePath, coordinate) {
      const { bridge, identity } = capture()
      const records = await bridge.getFileRecords(identity.sessionId, filePath)
      ensureCurrent(identity)
      if (records.revision !== identity.revision) throw new Error('插件查询的数据修订已过期')
      const row = records.records.find(item => sameCoordinate(item.coordinate, coordinate))
      return { ...identity, data: row ? projectRecord(row, true, filePath) : null }
    },
    async getField(filePath, coordinate, fieldPath) {
      if (fieldPath.length === 0 || fieldPath[0]?.kind !== 'field') {
        throw new Error('字段路径必须从顶层字段开始')
      }
      const { bridge, identity } = capture()
      const records = await bridge.getFileRecords(identity.sessionId, filePath)
      ensureCurrent(identity)
      if (records.revision !== identity.revision) throw new Error('插件查询的数据修订已过期')
      const row = records.records.find(item => sameCoordinate(item.coordinate, coordinate))
      return { ...identity, data: row ? cloneNullableValue(valueAtPath(row, fieldPath)) : null }
    },
    async searchRecords(query, options) {
      const normalized = query.trim()
      const mode = options?.mode ?? 'key'
      const limit = options?.limit ?? 200
      if (!normalized) throw new Error('搜索内容不能为空')
      if (mode !== 'key' && mode !== 'full_text') throw new Error('不支持的搜索模式')
      if (!Number.isInteger(limit) || limit <= 0) throw new Error('搜索结果上限必须是正整数')
      const { bridge, identity } = capture()
      const result = await bridge.searchRecords(identity.sessionId, normalized, mode, limit)
      ensureCurrent(identity)
      if (result.sessionId !== identity.sessionId || result.revision !== identity.revision) {
        throw new Error('插件查询的数据修订已过期')
      }
      return {
        ...identity,
        data: {
          hits: result.data.hits.map(cloneSearchHit),
          truncated: result.data.truncated,
        },
      }
    },
    mutate(request) {
      const cloned = cloneMutationRequest(request)
      const bridge = requireDataBridge()
      // 插件请求在 Data API 边界串行，保证每个请求独立进入历史事务。
      const operation = mutationQueue.then(async () => {
        if (dataBridge !== bridge) throw new Error('插件修改所属的项目会话已变更')
        const identity = bridge.currentIdentity()
        if (!identity) throw new Error('当前没有打开的项目')
        await bridge.mutate(cloned)
        const current = bridge.currentIdentity()
        if (!current || current.sessionId !== identity.sessionId || current.revision <= identity.revision) {
          throw new Error('插件修改未发布新的数据修订')
        }
        return current
      })
      mutationQueue = operation.then(() => undefined, () => undefined)
      return operation
    },
  }
}

function cloneSearchHit(hit: PluginSearchHit): PluginSearchHit {
  return {
    filePath: hit.filePath,
    coordinate: { ...hit.coordinate },
    fieldPath: hit.fieldPath,
    preview: hit.preview,
  }
}

function requireDataBridge(): PluginDataBridge {
  if (!dataBridge) throw new Error('插件数据 API 尚未连接编辑器')
  return dataBridge
}

function validateExactType(typeName: string): void {
  if (!typeName.trim() || typeName.includes('*')) throw new Error('必须提供明确的记录类型')
}

function projectRecord(row: RecordRow, includeFields: boolean, filePath = row.display_path): PluginRecordData {
  return {
    filePath,
    coordinate: { ...row.coordinate },
    ...(includeFields ? { fields: structuredClone(row.fields) } : {}),
  }
}

function valueAtPath(row: RecordRow, path: readonly FieldPathSegment[]): FieldValue | null {
  const first = path[0]
  if (!first || first.kind !== 'field') return null
  let value = row.fields.find(field => field.name === first.value)?.value
  for (const segment of path.slice(1)) {
    if (!value) return null
    while (value.kind === 'option_some' || value.kind === 'result_ok' || value.kind === 'result_err') value = value.value
    if (segment.kind === 'field' && value.kind === 'object') value = value.value.fields[segment.value]
    else if (segment.kind === 'index' && value.kind === 'array') value = value.value[segment.value]
    else if (segment.kind === 'dict_key' && value.kind === 'dict') {
      value = value.value.find(([key]) => dictKeyText(key) === segment.value)?.[1]
    } else return null
  }
  return value ?? null
}

function dictKeyText(key: DictKey): string {
  if (key.kind === 'int') return key.value.toString()
  if (key.kind === 'enum') {
    return key.value.variant
      ? `${key.value.enum_name}.${key.value.variant}`
      : `${key.value.enum_name}(${key.value.value})`
  }
  return `"${key.value
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t')}"`
}

function cloneNullableValue(value: FieldValue | null): FieldValue | null {
  return value ? cloneValue(value) : null
}

function cloneMutationRequest(request: PluginMutationRequest): PluginMutationRequest {
  return structuredClone(request)
}

function createHost(runtime: PluginRuntime): EditorPluginHost {
  const data = createDataApi()
  return {
    register: {
      page(contribution) {
        validateBase(contribution, '页面')
        if (typeof contribution.mount !== 'function') throw new Error('页面必须提供 mount')
        return registerContribution(runtime, contribution, runtime.pages)
      },
      view(contribution) {
        validateBase(contribution, '视图')
        validateTargets(contribution.types)
        if (typeof contribution.mount !== 'function') throw new Error('视图必须提供 mount')
        return registerContribution(runtime, { ...contribution, types: [...contribution.types] }, runtime.views)
      },
      sidebar(contribution) {
        validateBase(contribution, '侧栏')
        if (typeof contribution.mount !== 'function') throw new Error('侧栏必须提供 mount')
        return registerContribution(runtime, contribution, runtime.sidebars)
      },
      presentation(contribution) {
        validateId(contribution.id, '类型界面')
        validateTargets(contribution.types)
        if (contribution.slot === 'summary') {
          if (typeof contribution.render !== 'function') throw new Error('summary 类型界面必须提供 render')
        } else if ((contribution.slot === 'cell' || contribution.slot === 'inspector')
          && typeof contribution.mount !== 'function') {
          throw new Error(`${contribution.slot} 类型界面必须提供 mount`)
        } else if (contribution.slot !== 'cell' && contribution.slot !== 'inspector') {
          throw new Error('不支持的类型界面 slot')
        }
        return registerContribution(runtime, { ...contribution, types: [...contribution.types] }, runtime.presentations)
      },
      command(contribution) {
        validateBase(contribution, '命令')
        if (typeof contribution.run !== 'function') throw new Error('命令必须提供 run')
        return registerContribution(runtime, contribution, runtime.commands)
      },
      keybinding(contribution) {
        if (!contribution.command.trim()) throw new Error('按键绑定必须引用命令')
        const normalized = normalizeKeybinding(contribution.key)
        // 内部 ID 使用公开贡献 ID 不允许的前缀，且注销后不复用。
        const id = `@keybinding-${runtime.nextKeybindingId++}`
        return registerContribution(
          runtime,
          { ...contribution, id, accelerator: normalized },
          runtime.keybindings,
        )
      },
      openPage(pageId) {
        uiBridge?.openPage(runtime.id, pageId)
      },
      openSidebar(sidebarId) {
        uiBridge?.openSidebar(runtime.id, sidebarId)
      },
    },
    events: {
      on(event, listener) {
        const entry = { pluginId: runtime.id, listener } as EventListenerEntry<typeof event>
        const entries = eventListeners[event] as Set<typeof entry>
        entries.add(entry)
        const dispose = () => entries.delete(entry)
        runtime.eventDisposers.push(dispose)
        return dispose
      },
    },
    data,
  }
}

export async function loadFrontendPlugin(bundle: FrontendPluginBundle): Promise<void> {
  if (bundle.scope === 'project' ? !bundle.enabled : disabledGlobalIds.has(bundle.id)) {
    await activateFrontendPlugin(bundle, () => {})
    return
  }
  const url = URL.createObjectURL(new Blob([bundle.source], { type: 'text/javascript' }))
  try {
    const module = await import(/* @vite-ignore */ url) as { default?: EditorPluginActivate }
    if (typeof module.default !== 'function') throw new Error('插件必须默认导出 activate(host) 函数')
    const activated = await activateFrontendPlugin(bundle, module.default)
    if (activated) scriptUrls.set(bundle.id, url)
    else URL.revokeObjectURL(url)
  } catch (error) {
    URL.revokeObjectURL(url)
    throw error
  }
}

export async function activateFrontendPlugin(
  bundle: FrontendPluginBundle,
  activatePlugin: EditorPluginActivate,
): Promise<boolean> {
  validateId(bundle.id, '插件')
  unloadFrontendPlugin(bundle.id)
  const runtime = createPluginRuntime({
    id: bundle.id,
    name: bundle.name,
    description: bundle.description,
    version: bundle.version,
    origin: bundle.scope,
    manifestPath: bundle.manifest_path,
    enabled: bundle.scope === 'project' ? bundle.enabled : true,
  })
  if (!isEnabled(runtime)) {
    runtimes.push(runtime)
    persistDisabledGlobalIds()
    notify()
    return false
  }
  await activatePluginRuntime(runtime, activatePlugin)
  persistDisabledGlobalIds()
  return true
}

async function activateBuiltInPlugin(definition: BuiltInPluginDefinition): Promise<void> {
  validateId(definition.id, '插件')
  unloadFrontendPlugin(definition.id)
  const runtime = createPluginRuntime({
    id: definition.id,
    name: definition.name,
    description: definition.description,
    version: definition.version,
    origin: 'built-in',
    manifestPath: `built-in:${definition.id}`,
    enabled: true,
  })
  await activatePluginRuntime(runtime, definition.activate)
}

function createPluginRuntime(metadata: PluginMetadata): PluginRuntime {
  return {
    ...metadata,
    pages: [],
    views: [],
    sidebars: [],
    presentations: [],
    commands: [],
    keybindings: [],
    nextKeybindingId: 0,
    eventDisposers: [],
  }
}

async function activatePluginRuntime(
  runtime: PluginRuntime,
  activatePlugin: EditorPluginActivate,
): Promise<void> {
  try {
    const activated = await activatePlugin(createHost(runtime))
    runtime.dispose = activated?.dispose
    runtimes.push(runtime)
    notify()
  } catch (error) {
    runtime.eventDisposers.forEach(dispose => dispose())
    throw error
  }
}

export function unloadFrontendPlugin(id: string): void {
  const index = runtimes.findIndex(runtime => runtime.id === id)
  if (index < 0) return
  const [runtime] = runtimes.splice(index, 1)
  disposePluginRuntime(runtime)
  persistDisabledGlobalIds()
  notify()
}

function disposePluginRuntime(runtime: PluginRuntime): void {
  const keys = allContributionKeys(runtime)
  runtime.eventDisposers.forEach(dispose => dispose())
  try { runtime.dispose?.() } catch (error) { reportPluginError(runtime.name, error) }
  for (const key of keys) failedContributions.delete(key)
  runtime.pages.length = 0
  runtime.views.length = 0
  runtime.sidebars.length = 0
  runtime.presentations.length = 0
  runtime.commands.length = 0
  runtime.keybindings.length = 0
  runtime.eventDisposers.length = 0
  runtime.dispose = undefined
  const url = scriptUrls.get(runtime.id)
  if (url) URL.revokeObjectURL(url)
  scriptUrls.delete(runtime.id)
}

function allContributionKeys(runtime: PluginRuntime): string[] {
  return [
    ...runtime.pages,
    ...runtime.views,
    ...runtime.sidebars,
    ...runtime.presentations,
    ...runtime.commands,
    ...runtime.keybindings,
  ].map(item => item.key)
}

export function replaceFrontendPlugins(
  builtIns: readonly BuiltInPluginDefinition[],
  bundles: FrontendPluginBundle[],
): Promise<string[]> {
  const replacement = replacementQueue.then(
    () => replaceFrontendPluginsNow(builtIns, bundles),
    () => replaceFrontendPluginsNow(builtIns, bundles),
  )
  replacementQueue = replacement.then(() => undefined, () => undefined)
  return replacement
}

async function replaceFrontendPluginsNow(
  builtIns: readonly BuiltInPluginDefinition[],
  bundles: FrontendPluginBundle[],
): Promise<string[]> {
  for (const runtime of [...runtimes]) unloadFrontendPlugin(runtime.id)
  const errors: string[] = []
  const builtInIds = new Set<string>()
  for (const definition of builtIns) {
    if (builtInIds.has(definition.id)) {
      errors.push(`${definition.name}: 内置插件 id \`${definition.id}\` 重复`)
      continue
    }
    builtInIds.add(definition.id)
    try {
      await activateBuiltInPlugin(definition)
    } catch (error) {
      errors.push(`${definition.name}: ${errorMessage(error)}`)
    }
  }
  // 串行激活保证注册优先级只由配置顺序决定，与脚本加载耗时无关。
  for (const bundle of bundles) {
    if (builtInIds.has(bundle.id)) {
      errors.push(`${bundle.name}: 插件 id \`${bundle.id}\` 已由内置插件使用`)
      continue
    }
    try {
      await loadFrontendPlugin(bundle)
    } catch (error) {
      errors.push(`${bundle.name}: ${errorMessage(error)}`)
    }
  }
  return errors
}

export function setFrontendPluginEnabled(id: string, enabled: boolean): void {
  const runtime = runtimes.find(item => item.id === id)
  const wasEnabled = runtime ? isEnabled(runtime) : false
  if (runtime?.origin === 'built-in') return
  if (runtime?.origin === 'project') runtime.enabled = enabled
  if (runtime?.origin === 'global') {
    if (enabled) disabledGlobalIds.delete(id)
    else disabledGlobalIds.add(id)
  }
  // 禁用必须立即释放激活期资源；重新启用由宿主重载脚本完成。
  if (runtime && wasEnabled && !isEnabled(runtime)) disposePluginRuntime(runtime)
  persistDisabledGlobalIds()
  notify()
}

function persistDisabledGlobalIds(): void {
  try { localStorage.setItem(DISABLED_STORAGE_KEY, JSON.stringify([...disabledGlobalIds])) } catch { /* quota */ }
}

export function publishPluginEvent<K extends keyof PluginEventMap>(event: K, payload: PluginEventMap[K]): void {
  const entries = eventListeners[event] as Set<EventListenerEntry<K>>
  for (const entry of entries) {
    const runtime = runtimes.find(item => item.id === entry.pluginId)
    if (!runtime || !isEnabled(runtime)) continue
    try {
      Promise.resolve(entry.listener(structuredClone(payload)))
        .catch(error => reportPluginError(runtime.name, error))
    } catch (error) {
      reportPluginError(runtime.name, error)
    }
  }
}

export function usePluginViews(typeName: string): readonly RegisteredContribution<PluginViewContribution>[] {
  const registry = usePluginRegistry()
  return useMemo(
    () => registry.views.filter(view => view.types.includes(typeName)),
    [registry, typeName],
  )
}

export function preferredPluginView(typeName: string): RegisteredContribution<PluginViewContribution> | undefined {
  const registry = pluginRegistrySnapshot()
  const candidates = registry.views.filter(view => view.types.includes(typeName))
  const configured = registry.defaults.views[typeName]
  return candidates.find(view => view.key === configured)
    ?? candidates.find(view => view.default)
}

export function usePluginPresentation(
  slot: PluginPresentationSlot,
  typeName: string,
): RegisteredContribution<PluginPresentationContribution> | undefined {
  const registry = usePluginRegistry()
  return useMemo(() => resolvePluginPresentation(registry, slot, typeName), [registry, slot, typeName])
}

export function resolvePluginPresentation(
  registry: PluginRegistrySnapshot,
  slot: PluginPresentationSlot,
  typeName: string,
): RegisteredContribution<PluginPresentationContribution> | undefined {
  if (!typeName) return undefined
  const candidates = registry.presentations.filter(item => item.slot === slot && item.types.includes(typeName))
  const configured = registry.defaults.presentations[typeName]?.[slot]
  return candidates.find(item => item.key === configured)
    ?? candidates.find(item => item.default)
    ?? candidates[0]
}

export function failPluginContribution(key: string, error: unknown): void {
  if (failedContributions.has(key)) return
  failedContributions.add(key)
  const pluginName = runtimes.find(runtime => key.startsWith(`${runtime.id}/`))?.name ?? key
  reportPluginError(pluginName, error)
  notify()
}

function reportPluginError(pluginName: string, error: unknown): void {
  uiBridge?.reportError(`插件 ${pluginName} 运行失败：${errorMessage(error)}`)
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

type MountableContribution = RegisteredContribution<
  PluginPageContribution | PluginViewContribution | PluginSidebarContribution | PluginMountedPresentation
>
type MountContext = PluginPageContext | PluginViewContext | PluginSidebarContext | PluginPresentationContext

export function PluginContributionMount({
  contribution,
  context,
  fallback,
  inline = false,
  className,
  retainOnError = false,
}: {
  contribution: MountableContribution | undefined
  context: MountContext
  fallback?: ReactNode
  inline?: boolean
  className?: string
  retainOnError?: boolean
}) {
  const elementRef = useRef<HTMLElement>(null)
  const instanceRef = useRef<PluginUiInstance<MountContext> | null>(null)
  const mountedContextRef = useRef<MountContext | null>(null)
  const contextRef = useRef(context)
  contextRef.current = context
  const [localError, setLocalError] = useState(false)
  useEffect(() => setLocalError(false), [contribution?.key])
  useEffect(() => {
    if (localError) return
    const element = elementRef.current
    if (!contribution || !element) return
    const controller = new AbortController()
    try {
      const result = contribution.mount(contextRef.current as never, {
        element,
        signal: controller.signal,
        replace(content) { element.replaceChildren(content) },
      } satisfies PluginOutlet)
      instanceRef.current = typeof result === 'function'
        ? { dispose: result }
        : result ?? {}
      mountedContextRef.current = contextRef.current
    } catch (error) {
      controller.abort()
      element.replaceChildren()
      if (retainOnError) {
        reportPluginError(contribution.pluginId, error)
        setLocalError(true)
      } else {
        failPluginContribution(contribution.key, error)
      }
      return
    }
    return () => {
      controller.abort()
      try { instanceRef.current?.dispose?.() } catch (error) { reportPluginError(contribution.pluginId, error) }
      instanceRef.current = null
      mountedContextRef.current = null
      element.replaceChildren()
    }
  }, [contribution, localError, retainOnError])
  useEffect(() => {
    if (!contribution || localError || mountedContextRef.current === context) return
    try {
      instanceRef.current?.update?.(context)
      mountedContextRef.current = context
    } catch (error) {
      if (retainOnError) {
        reportPluginError(contribution.pluginId, error)
        setLocalError(true)
      } else {
        failPluginContribution(contribution.key, error)
      }
    }
  }, [context, contribution, localError, retainOnError])
  if (!contribution || localError) return <>{fallback}</>
  return createElement(inline ? 'span' : 'div', {
    ref: elementRef,
    className: className ?? 'plugin-contribution-host',
    'data-plugin-contribution': contribution.key,
  })
}

export function PluginSummaryText({
  presentation,
  context,
  fallback,
}: {
  presentation: RegisteredContribution<PluginSummaryPresentation> | undefined
  context: PluginPresentationContext
  fallback: ReactNode
}) {
  if (!presentation) return <>{fallback}</>
  try {
    const text = presentation.render(context)
    if (typeof text !== 'string') throw new Error('summary render 必须返回字符串')
    return <>{text}</>
  } catch (error) {
    queueMicrotask(() => failPluginContribution(presentation.key, error))
    return <>{fallback}</>
  }
}

export function openPluginPage(key: string): void {
  const page = snapshot.pages.find(item => item.key === key)
  if (page) uiBridge?.openPage(page.pluginId, page.id)
}

export async function executePluginCommand(
  key: string,
  activeContext: PluginActiveContext,
): Promise<boolean> {
  const command = snapshot.commands.find(item => item.key === key)
  if (!command) return false
  const context: PluginCommandContext = {
    ...activeContext,
    openPage: pageId => uiBridge?.openPage(command.pluginId, pageId),
  }
  try {
    await command.run(context)
  } catch (error) {
    reportPluginError(command.pluginId, error)
  }
  return true
}

export function dispatchPluginKeybinding(event: KeyboardEvent, context: PluginActiveContext): boolean {
  if (event.defaultPrevented || event.isComposing) return false
  const binding = snapshot.keybindings.find(item => {
    if (!keybindingMatchesEvent(item.accelerator, event)) return false
    try { return item.when?.(context) ?? true } catch (error) {
      reportPluginError(item.pluginId, error)
      return false
    }
  })
  if (!binding) return false
  const commandKey = binding.command.includes('/')
    ? binding.command
    : contributionKey(binding.pluginId, binding.command)
  if (!snapshot.commands.some(command => command.key === commandKey)) return false
  event.preventDefault()
  void executePluginCommand(commandKey, context)
  return true
}

function normalizeKeybinding(binding: string): string {
  const parts = binding.split('+').map(part => part.trim()).filter(Boolean)
  if (parts.length === 0) throw new Error('按键绑定不能为空')
  const key = parts.pop()!
  const modifiers = new Set(parts.map(part => part.toLowerCase()))
  if ([...modifiers].some(part => !['mod', 'ctrl', 'meta', 'alt', 'shift'].includes(part))) {
    throw new Error(`无法识别按键绑定 \`${binding}\``)
  }
  return [
    modifiers.has('mod') ? 'Mod' : null,
    modifiers.has('ctrl') ? 'Ctrl' : null,
    modifiers.has('meta') ? 'Meta' : null,
    modifiers.has('alt') ? 'Alt' : null,
    modifiers.has('shift') ? 'Shift' : null,
    normalizeKey(key),
  ].filter(Boolean).join('+')
}

function keybindingMatchesEvent(binding: string, event: KeyboardEvent): boolean {
  const parts = binding.split('+')
  const key = parts[parts.length - 1]
  const modifiers = new Set(parts.slice(0, -1))
  const usesMod = modifiers.has('Mod')
  if (usesMod && !event.ctrlKey && !event.metaKey) return false
  const ctrl = modifiers.has('Ctrl') || (usesMod && event.ctrlKey)
  const meta = modifiers.has('Meta') || (usesMod && event.metaKey)
  return key === normalizeKey(event.key)
    && event.ctrlKey === ctrl
    && event.metaKey === meta
    && event.altKey === modifiers.has('Alt')
    && event.shiftKey === modifiers.has('Shift')
}

function normalizeKey(key: string): string {
  if (key === ' ') return 'Space'
  return key.length === 1 ? key.toUpperCase() : key
}

export function resetPluginRegistryForTests(): void {
  for (const runtime of [...runtimes]) unloadFrontendPlugin(runtime.id)
  for (const entries of Object.values(eventListeners)) entries.clear()
  failedContributions.clear()
  projectDefaults = EMPTY_DEFAULTS
  dataBridge = null
  uiBridge = null
  disabledGlobalIds = new Set()
  replacementQueue = Promise.resolve()
  mutationQueue = Promise.resolve()
  notify()
}
