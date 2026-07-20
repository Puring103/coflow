import { useState, useEffect, useCallback, useMemo, useRef, useSyncExternalStore } from 'react'
import { FileTree } from './components/FileTree'
import { TableView } from './components/TableView'
import { RecordView } from './components/RecordView'
import { GraphView } from './components/GraphView'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { InspectorPanel } from './components/InspectorPanel'
import { Icon } from './components/Icon'
import { ObjectDraftHost } from './components/ObjectDraftHost'
import { UpdateControl } from './components/UpdateControl'
import { DimensionTableView } from './components/DimensionTableView'
import { useRouter } from './hooks/useRouter'
import { useTheme } from './hooks/useTheme'
import {
  loadLocalReadPlugin,
  restoreLocalReadPlugins,
  setReadPluginEnabled,
  unloadLocalReadPlugin,
  useReadPluginSettings,
} from './plugins'
import {
  MOCK_PROJECT,
  MOCK_FILE_RECORDS,
  MOCK_GRAPH,
  MOCK_DIMENSION_FILE_RECORDS,
  MOCK_EDITOR_SETTINGS,
} from './mock'
import * as api from './api'
import type { DimensionInfo } from './bindings/DimensionInfo'
import type { DimensionValueCoordinate } from './bindings/DimensionValueCoordinate'
import type { DimensionValueState } from './bindings/DimensionValueState'
import type { FileRecords } from './bindings/FileRecords'
import type { EditorProjectSettings } from './bindings/EditorProjectSettings'
import type { EditorRecordGroup } from './bindings/EditorRecordGroup'
import type { CreateRecordDraft } from './bindings/CreateRecordDraft'
import type { GraphData } from './bindings/GraphData'
import type { ProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import type { RecordRow } from './bindings/RecordRow'
import type { WriterCapabilities } from './bindings/WriterCapabilities'
import {
  diagnosticKey,
  diagnosticMatchesAnchor,
  errorDiagnostics,
  errorMessage,
  cloneValue,
  recordActualType,
  recordKey,
    coordinateId,
    sameCoordinate,
  type FieldPathSegment,
  type FieldValue,
} from './wire'
import { recordMatchesSearch } from './value/fieldValue'
import { isEditableFile } from './utils/editable'
import { EditorLookupController } from './state/editorLookups'
import {
  MutationHistoryController,
  publishMutationGeneration,
  ProjectGenerationController,
  type MutationPublicationRequest,
} from './state/editorState'
import {
  EditorMutationController,
  type EditorMutationPort,
} from './state/editorMutations'
import { historyShortcutFor } from './state/editorShortcuts'
import { projectFieldValue, projectFieldValueAtRevision } from './state/fieldProjection'
import {
  recordSelection,
  rebindSelection,
  removeSelection,
  updateRecordSelection,
  valueSelection,
  type EditorSelection,
  type RecordSelectionMode,
} from './state/editorSelection'
import {
  createRecordGroup,
  moveRecordsOntoRecord,
  moveRecordsToGroup,
  nextRecordGroupName,
  colorRecordGroup,
  removeRecordFromGroups,
  removeRecordsFromGroups,
  renameRecordGroup,
  replaceGroupedCoordinate,
} from './state/manualRecordGroups'
import { recordsSupportGraph } from './state/graphSupport'
import './style.css'

const GRAPH_DEPTH = 3
const GRAPH_LIMIT = 1_000
const LAST_PROJECT_STORAGE_KEY = 'cfd-editor-last-project-yaml'

interface WorkspaceTab {
  id: string
  filePath: string
  typeName: string
}
function workspaceTabId(filePath: string, typeName: string): string {
  return `${filePath}\u001f${typeName}`
}

function settingsWithRecordGroups(
  settings: EditorProjectSettings | null,
  filePath: string,
  actualType: string,
  groups: EditorRecordGroup[],
): EditorProjectSettings {
  return {
    table_column_widths: settings?.table_column_widths ?? {},
    graph_enabled_fields: settings?.graph_enabled_fields ?? {},
    record_groups: {
      ...(settings?.record_groups ?? {}),
      [filePath]: {
        ...(settings?.record_groups?.[filePath] ?? {}),
        [actualType]: groups,
      },
    },
  }
}

function settingsWithGraphFields(
  settings: EditorProjectSettings | null,
  filePath: string,
  actualType: string,
  fields: string[],
): EditorProjectSettings {
  return {
    table_column_widths: settings?.table_column_widths ?? {},
    record_groups: settings?.record_groups ?? {},
    graph_enabled_fields: {
      ...(settings?.graph_enabled_fields ?? {}),
      [filePath]: {
        ...(settings?.graph_enabled_fields?.[filePath] ?? {}),
        [actualType]: fields,
      },
    },
  }
}

/** Passed as `highlightField` when a record-level (no field path) jump lands
 *  on a record view — RecordView flashes the CardHeader instead of a row. */
export const RECORD_HIGHLIGHT_SENTINEL = '__record__'

function graphCacheKey(
  filePath: string,
  depth: number,
  limit: number,
): string {
  return `${filePath}::${depth}::${limit}`
}

function projectGraphRows(
  cache: Record<string, GraphData>,
  revision: number,
  rows: RecordRow[],
): Record<string, GraphData> {
  const rowByCoordinate = new Map(
    rows.map(row => [`${row.coordinate.actual_type}\u001f${row.coordinate.key}`, row]),
  )
  let changed = false
  const next: Record<string, GraphData> = {}
  for (const [key, graph] of Object.entries(cache)) {
    if (graph.revision !== revision - 1 && graph.revision !== revision) {
      next[key] = graph
      continue
    }
    const nodes = graph.nodes.map(node => {
      const row = rowByCoordinate.get(
        `${node.coordinate.actual_type}\u001f${node.coordinate.key}`,
      )
      if (!row) return node
      return {
        ...node,
        fields: row.fields,
        field_diagnostics: row.field_diagnostics,
        diagnostic_severity: row.diagnostic_severity,
      }
    })
    const projected = graph.revision === revision && nodes.every((node, index) => node === graph.nodes[index])
      ? graph
      : { ...graph, revision, nodes }
    if (projected !== graph) changed = true
    next[key] = projected
  }
  return changed ? next : cache
}

export default function App() {
  const pluginSettings = useReadPluginSettings()
  const restoredPlugins = useRef(false)
  const [pluginLoadBusy, setPluginLoadBusy] = useState(false)
  const [pluginLoadError, setPluginLoadError] = useState<string | null>(null)
  const [project, setProject] = useState<ProjectSnapshot | null>(null)
  useEffect(() => {
    const suppressBrowserMenu = (event: MouseEvent) => event.preventDefault()
    window.addEventListener('contextmenu', suppressBrowserMenu)
    return () => window.removeEventListener('contextmenu', suppressBrowserMenu)
  }, [])
  const [generation] = useState(() => new ProjectGenerationController())
  const [history] = useState(() => new MutationHistoryController())
  const [lookups] = useState(() => new EditorLookupController(api))
  const lookupGenerationKey = project ? `${project.session_id}:${project.revision}` : 'none'
  const historySnapshot = useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot)
  const [fileDataCache, setFileDataCache] = useState<Record<string, FileRecords>>({})
  const [dimensionFileCache, setDimensionFileCache] = useState<Record<string, api.DimensionFileRecords>>({})
  const [projectDimensions, setProjectDimensions] = useState<DimensionInfo[]>([])
  const [dimensionView, setDimensionView] = useState<'table' | 'record'>('table')
  const [graphCache, setGraphCache] = useState<Record<string, GraphData>>({})
  const [projectSettings, setProjectSettings] = useState<EditorProjectSettings | null>(null)
  const fileDataCacheRef = useRef(fileDataCache)
  const graphCacheRef = useRef(graphCache)
  fileDataCacheRef.current = fileDataCache
  graphCacheRef.current = graphCache
  const [showHelp, setShowHelp] = useState(false)
  const helpBoxRef = useRef<HTMLDivElement>(null)
  const helpReturnRef = useRef<HTMLElement | null>(null)
  const [loadingFile, setLoadingFile] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [projectAction, setProjectAction] = useState<'build' | null>(null)
  const [projectActionNotice, setProjectActionNotice] = useState<{
    message: string
    tone: 'success' | 'error'
  } | null>(null)

  const router = useRouter()
  const { theme, toggle: toggleTheme } = useTheme()
  const [activeType, setActiveType] = useState<string>('')
  const [workspaceTabs, setWorkspaceTabs] = useState<WorkspaceTab[]>([])
  const [activeWorkspaceTabId, setActiveWorkspaceTabId] = useState<string | null>(null)
  // The last view the user actively picked. `openFile` pushes a table
  // placeholder because record view needs a coordinate we don't yet have;
  // once the file data lands, the effect below upgrades the route to
  // `preferredView` if that's not what we currently show.
  const [preferredView, setPreferredView] = useState<'table' | 'record' | 'graph'>('table')
  const [activePane, setActivePane] = useState<'files' | 'search' | 'extensions' | 'ai'>(() => {
    try {
      const v = localStorage.getItem('cfd-editor-active-pane')
      return v === 'search' || v === 'extensions' || v === 'ai' ? v : 'files'
    } catch { return 'files' }
  })
  const [settingsOpen, setSettingsOpen] = useState(false)
  const settingsMenuRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    try { localStorage.setItem('cfd-editor-active-pane', activePane) } catch { /* quota */ }
  }, [activePane])
  useEffect(() => {
    if (!settingsOpen) return
    const onClick = (e: MouseEvent) => {
      if (!settingsMenuRef.current?.contains(e.target as Node)) setSettingsOpen(false)
    }
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [settingsOpen])
  useEffect(() => {
    if (!api.isTauri || restoredPlugins.current) return
    restoredPlugins.current = true
    api.listFrontendPlugins().then(restoreLocalReadPlugins).then(errors => {
      if (errors.length > 0) setPluginLoadError(`部分插件未加载：${errors.join('; ')}`)
    })
  }, [])
  const loadPluginFromSettings = useCallback(async () => {
    const manifestPath = await api.pickFrontendPluginManifest()
    if (!manifestPath) return
    setPluginLoadBusy(true)
    setPluginLoadError(null)
    try {
      await loadLocalReadPlugin(await api.installFrontendPlugin(manifestPath))
    } catch (error) {
      setPluginLoadError(`加载插件失败：${errorMessage(error)}`)
    } finally {
      setPluginLoadBusy(false)
    }
  }, [])
  const uninstallPluginFromSettings = useCallback(async (id: string) => {
    setPluginLoadError(null)
    try {
      await api.uninstallFrontendPlugin(id)
      unloadLocalReadPlugin(id)
    } catch (error) {
      setPluginLoadError(`卸载插件失败：${errorMessage(error)}`)
    }
  }, [])
  const [tabOverflowOpen, setTabOverflowOpen] = useState(false)
  const [tabsOverflow, setTabsOverflow] = useState(false)
  const tabScrollRef = useRef<HTMLDivElement>(null)
  const tabOverflowRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!tabOverflowOpen) return
    const onClick = (e: MouseEvent) => {
      if (!tabOverflowRef.current?.contains(e.target as Node)) setTabOverflowOpen(false)
    }
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [tabOverflowOpen])
  // Scroll the active tab into view when it changes.
  useEffect(() => {
    if (!activeWorkspaceTabId) return
    const el = tabScrollRef.current?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(activeWorkspaceTabId)}"]`)
    el?.scrollIntoView({ inline: 'nearest', block: 'nearest' })
  }, [activeWorkspaceTabId])
  // Track whether tabs actually overflow their container so we only surface the
  // dropdown when needed. ResizeObserver reacts to sidebar / inspector resizes;
  // scrollWidth changes when tabs open/close are handled by the workspaceTabs dep.
  useEffect(() => {
    const el = tabScrollRef.current
    if (!el) { setTabsOverflow(false); return }
    const check = () => setTabsOverflow(el.scrollWidth > el.clientWidth + 1)
    check()
    const ro = new ResizeObserver(check)
    ro.observe(el)
    for (const child of Array.from(el.children)) ro.observe(child)
    return () => ro.disconnect()
  }, [workspaceTabs])
  const [globalSearch, setGlobalSearch] = useState('')
  const [collapsedRecordGroups, setCollapsedRecordGroups] = useState<Set<string>>(() => new Set())
  const recordGroupIdSequence = useRef(0)
  const recordGroupSaveSequence = useRef(0)
  const graphFieldsSaveSequence = useRef(0)
  const globalSearchRef = useRef<HTMLInputElement>(null)
  const sidebarRef = useRef<HTMLDivElement>(null)
  const viewContainerRef = useRef<HTMLDivElement>(null)
  const [inspectorCollapsed, setInspectorCollapsed] = useState(false)
  const [inspectorFocusRequest, setInspectorFocusRequest] = useState(0)
  const [tableFocusRequest, setTableFocusRequest] = useState(0)
  const [firstRecordFocusRequest, setFirstRecordFocusRequest] = useState(0)
  const startupProjectRequested = useRef(false)
  // Field path to briefly highlight after a diagnostic jump. Cleared after
  // the RecordView applies the highlight so subsequent navigations don't
  // re-flash it.
  const [highlightField, setHighlightField] = useState<string | null>(null)
  // Diagnostics panel focus: which item (by stable key) should be revealed
  // and pulsed. Set from either the panel itself (self-scroll) or from a
  // record/field corner badge click. Consumed by DiagnosticsPanel; we bump
  // `diagFocusTick` so repeat clicks on the same badge re-flash the item.
  const [diagFocus, setDiagFocus] = useState<{ key: string; tick: number } | null>(null)

  const saveRecordGroups = useCallback((
    filePath: string,
    actualType: string,
    groups: EditorRecordGroup[],
  ) => {
    const sequence = ++recordGroupSaveSequence.current
    setProjectSettings(current => settingsWithRecordGroups(current, filePath, actualType, groups))
    if (!api.isTauri) return
    const identity = generation.currentIdentity()
    if (!identity) return
    api.setRecordGroups(identity.sessionId, filePath, actualType, groups)
      .then(settings => {
        if (generation.currentSession() === identity.sessionId
          && recordGroupSaveSequence.current === sequence) {
          setProjectSettings(settings)
        }
      })
      .catch(error => {
        if (generation.currentSession() === identity.sessionId) {
          setErrorMsg(`保存记录分组失败: ${errorMessage(error)}`)
        }
      })
  }, [generation])

  const saveGraphEnabledFields = useCallback((
    filePath: string,
    actualType: string,
    fields: string[],
  ) => {
    const sequence = ++graphFieldsSaveSequence.current
    setProjectSettings(current => settingsWithGraphFields(current, filePath, actualType, fields))
    if (!api.isTauri) return
    const identity = generation.currentIdentity()
    if (!identity) return
    api.setGraphEnabledFields(identity.sessionId, filePath, actualType, fields)
      .then(settings => {
        if (generation.currentSession() === identity.sessionId
          && graphFieldsSaveSequence.current === sequence) {
          setProjectSettings(settings)
        }
      })
      .catch(error => {
        if (generation.currentSession() === identity.sessionId) {
          setErrorMsg(`保存图谱字段失败: ${errorMessage(error)}`)
        }
      })
  }, [generation])

  // Resizable sidebar width, persisted to localStorage.
  const [sidebarW, setSidebarW] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('cfd-editor-sidebar-w') : null
    const n = raw ? parseInt(raw, 10) : NaN
    return Number.isFinite(n) ? Math.min(480, Math.max(160, n)) : 220
  })
  const [splitterDragging, setSplitterDragging] = useState(false)

  // Right-side inspector panel: table cells select one value, while the Key
  // column and graph nodes select the whole record.
  const [inspectorSelection, setInspectorSelection] = useState<EditorSelection | null>(null)
  const inspectorOpen = inspectorSelection !== null
  const inspectorCoord = useMemo(() => inspectorSelection
    ? { file: inspectorSelection.filePath, coordinate: inspectorSelection.coordinate }
    : null,
  [inspectorSelection])
  const [inspectorW, setInspectorW] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('cfd-editor-inspector-w') : null
    const n = raw ? parseInt(raw, 10) : NaN
    return Number.isFinite(n) ? Math.min(720, Math.max(320, n)) : 420
  })
  useEffect(() => {
    try { localStorage.setItem('cfd-editor-inspector-w', String(inspectorW)) } catch { /* quota */ }
  }, [inspectorW])
  const openInspector = useCallback((file: string, coordinate: RecordCoordinate) => {
    setInspectorSelection(prev => {
      if (prev?.kind === 'record' && prev.filePath === file && sameCoordinate(prev.coordinate, coordinate)) return prev
      return recordSelection(file, coordinate)
    })
  }, [])
  const openValueInspector = useCallback((
    file: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
  ) => {
    setInspectorCollapsed(false)
    setInspectorSelection(valueSelection(file, coordinate, fieldPath))
  }, [])
  const closeInspector = useCallback(() => setInspectorSelection(null), [])

  // Auto-load mock data only when not running in Tauri (browser preview).
  useEffect(() => {
    if (!api.isTauri) {
      generation.adopt(MOCK_PROJECT)
      lookups.adopt({ sessionId: MOCK_PROJECT.session_id, revision: MOCK_PROJECT.revision })
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setProjectSettings(MOCK_EDITOR_SETTINGS)
      setProjectDimensions(MOCK_PROJECT.dimensions)
      setGraphCache({ [graphCacheKey('data/npc.cfd', GRAPH_DEPTH, GRAPH_LIMIT)]: MOCK_GRAPH })
      if (MOCK_PROJECT.first_source_file) {
        const filePath = MOCK_PROJECT.first_source_file
        const typeName = MOCK_PROJECT.file_types[filePath]?.[0]?.name ?? ''
        const tab = { id: workspaceTabId(filePath, typeName), filePath, typeName }
        setWorkspaceTabs([tab])
        setActiveWorkspaceTabId(tab.id)
        setActiveType(typeName)
        router.push({ view: 'table', file: filePath, typeFilter: typeName })
      }
    }
  }, [generation, lookups, router.push])

  // Reset all per-session UI state to a clean slate before swapping in a
  // new project snapshot. Used by both "open" and "new" flows so behavior
  // is identical. Also closes the previous backend session so the
  // SessionStore doesn't accumulate stale sessions across project switches.
  const adoptSnapshot = useCallback(
    (snapshot: ProjectSnapshot) => {
      generation.adopt(snapshot)
      lookups.adopt({ sessionId: snapshot.session_id, revision: snapshot.revision })
      setProject(prev => {
        // Fire-and-forget close of the outgoing session. We read prev here
        // (not `project` from the closure) so we always close exactly the
        // session we're replacing, even if state was stale at call time.
        if (prev && api.isTauri && prev.session_id !== snapshot.session_id) {
          api.closeSession(prev.session_id).catch(() => { /* best-effort */ })
        }
        return snapshot
      })
      setFileDataCache({})
      setDimensionFileCache({})
      setGraphCache({})
      setProjectSettings(api.isTauri ? null : MOCK_EDITOR_SETTINGS)
      setProjectDimensions(api.isTauri ? [] : MOCK_PROJECT.dimensions)
      setWorkspaceTabs([])
      setActiveWorkspaceTabId(null)
      setActiveType('')
      history.clear()
      const firstFile = snapshot.first_source_file ?? collectSourceFiles(snapshot)[0]
      if (firstFile) {
        const typeName = snapshot.file_types[firstFile]?.[0]?.name ?? ''
        const tab = { id: workspaceTabId(firstFile, typeName), filePath: firstFile, typeName }
        setWorkspaceTabs([tab])
        setActiveWorkspaceTabId(tab.id)
        setActiveType(typeName)
        router.push({ view: 'table', file: firstFile, typeFilter: typeName })
      } else {
        router.clear()
      }
      if (api.isTauri) {
        api.getProjectSettings(snapshot.session_id).then(settings => {
          if (generation.currentSession() === snapshot.session_id) setProjectSettings(settings)
        }).catch(err => {
          if (generation.currentSession() === snapshot.session_id) {
            setErrorMsg(`读取编辑器设置失败: ${errorMessage(err)}`)
          }
        })
        api.getProjectDimensions(snapshot.session_id).then(dimensions => {
          if (generation.currentSession() === snapshot.session_id) setProjectDimensions(dimensions)
        }).catch(err => {
          if (generation.currentSession() === snapshot.session_id) {
            setErrorMsg(`读取维度配置失败: ${errorMessage(err)}`)
          }
        })
      }
    },
    [generation, history, lookups, router]
  )

  useEffect(() => {
    if (!api.isTauri || startupProjectRequested.current) return
    startupProjectRequested.current = true
    const yamlPath = readLastProjectPath()
    if (!yamlPath) return
    const request = generation.beginProjectRequest()
    api.loadProject(yamlPath).then(snapshot => {
      if (!generation.isProjectRequestCurrent(request)) return
      adoptSnapshot(snapshot)
    }).catch(err => {
      if (generation.isProjectRequestCurrent(request)) {
        setErrorMsg(`自动打开上次项目失败: ${errorMessage(err)}`)
      }
    })
  }, [adoptSnapshot, generation])

  const reportSessionError = useCallback((
    sessionId: number,
    prefix: string,
    err: unknown,
    includeDiagnostics = false,
    expectedRevision?: number,
  ) => {
    if (
      generation.currentSession() !== sessionId
      || (expectedRevision !== undefined && !generation.isCurrent(sessionId, expectedRevision))
    ) return
    setErrorMsg(`${prefix}: ${errorMessage(err)}`)
    if (!includeDiagnostics) return
    const diagnostics = errorDiagnostics(err)
    if (diagnostics.length === 0) return
    setProject(current => (
      current?.session_id === sessionId
        ? { ...current, diagnostics: [...current.diagnostics, ...diagnostics] }
        : current
    ))
  }, [generation])

  const openProject = useCallback(async () => {
    if (!api.isTauri) {
      generation.adopt(MOCK_PROJECT)
      lookups.adopt({ sessionId: MOCK_PROJECT.session_id, revision: MOCK_PROJECT.revision })
      history.clear()
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setProjectSettings(MOCK_EDITOR_SETTINGS)
      return
    }
    const request = generation.beginProjectRequest()
    const yamlPath = await api.pickProjectYaml()
    if (!generation.isProjectRequestCurrent(request) || !yamlPath) return
    setErrorMsg(null)
    try {
      const snapshot = await api.loadProject(yamlPath)
      if (!generation.isProjectRequestCurrent(request)) return
      rememberLastProject(yamlPath)
      adoptSnapshot(snapshot)
    } catch (err) {
      if (!generation.isProjectRequestCurrent(request)) return
      setErrorMsg(`打开项目失败: ${errorMessage(err)}`)
      const diags = errorDiagnostics(err)
      if (diags.length > 0) {
        setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
      }
    }
  }, [adoptSnapshot, generation, history, lookups])

  const refreshFromSnapshot = useCallback(
    async (snapshot: ProjectSnapshot) => {
      if (!generation.acceptSnapshot(snapshot)) return
      lookups.adopt({ sessionId: snapshot.session_id, revision: snapshot.revision })
      const current = router.current
      const sourceFiles = collectSourceFiles(snapshot)
      const keepFile = current && sourceFiles.includes(current.file)
      const nextFile = keepFile ? current.file : sourceFiles[0]
      history.clear()
      setHighlightField(null)
      if (!nextFile) {
        setProject(snapshot)
        setFileDataCache({})
        setGraphCache({})
        return
      }
      if (!current || !keepFile) {
        setProject(snapshot)
        const typeName = snapshot.file_types[nextFile]?.[0]?.name ?? ''
        const tab = { id: workspaceTabId(nextFile, typeName), filePath: nextFile, typeName }
        setWorkspaceTabs(existing => [
          ...existing.filter(item => sourceFiles.includes(item.filePath) && item.id !== tab.id),
          tab,
        ])
        setActiveWorkspaceTabId(tab.id)
        setActiveType(typeName)
        router.push({ view: 'table', file: nextFile, typeFilter: typeName })
        return
      }
      try {
        const fileRecords = api.isTauri
          ? await api.getFileRecords(snapshot.session_id, nextFile)
          : null
        if (
          !generation.isCurrent(snapshot.session_id, snapshot.revision) ||
          (fileRecords && fileRecords.revision !== snapshot.revision)
        ) return
        setProject(snapshot)
        if (fileRecords) {
          setFileDataCache(cache => ({ ...cache, [nextFile]: fileRecords }))
        }
        if (current.view === 'record') {
          const stillExists = fileRecords?.records.some(r => sameCoordinate(r.coordinate, current.coordinate)) ?? false
          router.replace(stillExists
            ? current
            : {
                view: 'table',
                file: nextFile,
                typeFilter: current.coordinate.actual_type,
              })
        } else {
          router.replace(current)
        }
      } catch (err) {
        if (generation.isCurrent(snapshot.session_id, snapshot.revision)) {
          setProject(snapshot)
          reportSessionError(snapshot.session_id, '刷新项目失败', err)
          router.push({ view: 'table', file: nextFile, typeFilter: snapshot.file_types[nextFile]?.[0]?.name ?? '' })
        }
      }
    },
    [generation, history, lookups, reportSessionError, router],
  )

  const commitProjectRevision = useCallback((
    sessionId: number,
    revision: number,
    diagnostics: ProjectSnapshot['diagnostics'],
  ) => {
    if (!generation.acceptMutation(sessionId, revision)) return false
    lookups.adopt({ sessionId, revision })
    setProject(current => (
      current && current.session_id === sessionId && current.revision <= revision
        ? { ...current, revision, diagnostics }
        : current
    ))
    return true
  }, [generation, lookups])

  const publishMutation = useCallback((request: MutationPublicationRequest) => (
    publishMutationGeneration({
      acceptRevision: commitProjectRevision,
      isCurrent: (sessionId, revision) => generation.isCurrent(sessionId, revision),
      getFileRecords: api.getFileRecords,
      publishFileRecords: records => {
        setFileDataCache(current => {
          const next = { ...current }
          for (const [file, fileRecords] of records) next[file] = fileRecords
          fileDataCacheRef.current = next
          return next
        })
      },
      publishGraphProjection: (revision, records, topologyChanged) => {
        if (topologyChanged) return
        setGraphCache(current => {
          const next = projectGraphRows(
            current,
            revision,
            records.flatMap(file => file.records),
          )
          graphCacheRef.current = next
          return next
        })
      },
    }, request)
  ), [commitProjectRevision, generation])

  useEffect(() => {
    if (!api.isTauri || !project) return
    const sessionId = project.session_id
    let disposed = false
    let unlistenChanged: (() => void) | null = null
    let unlistenError: (() => void) | null = null
    const isCurrent = () => !disposed && generation.currentSession() === sessionId
    api.onProjectChanged(event => {
      if (!isCurrent() || event.session_id !== sessionId) return
      refreshFromSnapshot(event.snapshot).catch(err => {
        if (isCurrent()) reportSessionError(sessionId, '刷新项目失败', err)
      })
    }).then(unlisten => {
      if (!isCurrent()) unlisten()
      else unlistenChanged = unlisten
    }).catch(err => {
      if (isCurrent()) reportSessionError(sessionId, '监听项目变更失败', err)
    })
    api.onProjectWatchError(event => {
      if (!isCurrent() || event.session_id !== sessionId) return
      setErrorMsg(`监听项目变更失败: ${event.message}`)
    }).then(unlisten => {
      if (!isCurrent()) unlisten()
      else unlistenError = unlisten
    }).catch(err => {
      if (isCurrent()) reportSessionError(sessionId, '监听项目变更失败', err)
    })
    return () => {
      disposed = true
      unlistenChanged?.()
      unlistenError?.()
    }
  }, [generation, project?.session_id, refreshFromSnapshot, reportSessionError])

  // "新建工程": pick an empty directory, scaffold a minimal Coflow
  // project (mirrors `coflow init`), and open it. The same back-end call
  // refuses to clobber an existing `coflow.yaml` and that diagnostic
  // surfaces here as a clear error banner.
  const newProject = useCallback(async () => {
    if (!api.isTauri) {
      setErrorMsg('新建工程仅在桌面环境可用')
      return
    }
    const request = generation.beginProjectRequest()
    const dir = await api.pickProjectDirectory()
    if (!generation.isProjectRequestCurrent(request) || !dir) return
    setErrorMsg(null)
    try {
      const snapshot = await api.initProject(dir)
      if (!generation.isProjectRequestCurrent(request)) return
      rememberLastProject(projectYamlPath(dir))
      adoptSnapshot(snapshot)
    } catch (err) {
      if (!generation.isProjectRequestCurrent(request)) return
      setErrorMsg(`新建工程失败: ${errorMessage(err)}`)
      const diags = errorDiagnostics(err)
      if (diags.length > 0) {
        setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
      }
    }
  }, [adoptSnapshot, generation])

  // Lazy-load file records when navigated to
  useEffect(() => {
    if (!project || !router.current) return
    const file = router.current.file
    if (dimensionForFile(projectDimensions, file)) return
    if (fileDataCache[file]?.revision === project.revision) return
    if (!api.isTauri) return // mock branch already populated
    const sessionId = project.session_id
    const revision = project.revision
    const request = generation.captureRequest()
    setLoadingFile(file)
    api
      .getFileRecords(sessionId, file)
      .then(records => {
        if (
          !generation.isCurrent(sessionId, revision) ||
          records.revision !== revision
        ) return
        setFileDataCache(c => ({ ...c, [file]: records }))
      })
      .catch(err => {
        if (generation.isRequestCurrent(request)) {
          reportSessionError(sessionId, '读取文件失败', err)
        }
      })
      .finally(() => {
        if (generation.isRequestCurrent(request)) setLoadingFile(null)
      })
  }, [generation, project, projectDimensions, router.current, fileDataCache, reportSessionError])

  useEffect(() => {
    if (!project || !router.current) return
    const file = router.current.file
    if (!dimensionForFile(projectDimensions, file)) return
    if (dimensionFileCache[file]?.revision === project.revision) return
    if (!api.isTauri) {
      const mock = MOCK_DIMENSION_FILE_RECORDS[file]
      if (mock) setDimensionFileCache(cache => ({ ...cache, [file]: mock }))
      return
    }
    const sessionId = project.session_id
    const revision = project.revision
    const request = generation.captureRequest()
    setLoadingFile(file)
    api.getDimensionFileRecords(sessionId, file)
      .then(records => {
        if (!generation.isCurrent(sessionId, revision) || records.revision !== revision) return
        setDimensionFileCache(cache => ({ ...cache, [file]: records }))
      })
      .catch(error => {
        if (generation.isRequestCurrent(request)) {
          reportSessionError(sessionId, '读取维度文件失败', error)
        }
      })
      .finally(() => {
        if (generation.isRequestCurrent(request)) setLoadingFile(null)
      })
  }, [dimensionFileCache, generation, project, projectDimensions, reportSessionError, router.current])

  // Lazy-load graph when switching to graph view
  useEffect(() => {
    if (!project || router.current?.view !== 'graph') return
    const file = router.current.file
    const key = graphCacheKey(file, GRAPH_DEPTH, GRAPH_LIMIT)
    if (graphCache[key]?.revision === project.revision) return
    if (!api.isTauri) {
      setGraphCache(c => ({ ...c, [key]: MOCK_GRAPH }))
      return
    }
    let cancelled = false
    const sessionId = project.session_id
    const revision = project.revision
    const request = generation.captureRequest()
    api
      .getGraph(sessionId, file, {
        depth: GRAPH_DEPTH,
        limit: GRAPH_LIMIT,
      })
      .then(g => {
        if (
          !cancelled &&
          generation.isCurrent(sessionId, revision) &&
          g.revision === revision
        ) setGraphCache(c => ({ ...c, [key]: g }))
      })
      .catch(err => {
        if (!cancelled && generation.isRequestCurrent(request)) {
          reportSessionError(sessionId, '读取图谱失败', err)
        }
      })
    return () => { cancelled = true }
  }, [generation, project, router.current, graphCache, reportSessionError])

  // Auto-collapse inspector when switching to record view; restore for table/graph.
  useEffect(() => {
    const view = router.current?.view
    if (view === 'record') {
      setInspectorCollapsed(true)
    } else if (view === 'table' || view === 'graph') {
      setInspectorCollapsed(false)
    }
  }, [router.current?.view])

  const openFile = useCallback(
    (filePath: string, requestedType = '') => {
      setGlobalSearch('')
      const typeName = requestedType || project?.file_types[filePath]?.[0]?.name || ''
      const id = workspaceTabId(filePath, typeName)
      setWorkspaceTabs(current => current.some(tab => tab.id === id)
        ? current
        : [...current, { id, filePath, typeName }])
      setActiveWorkspaceTabId(id)
      setActiveType(typeName)
      const currentView = router.current?.view ?? 'table'
      if (currentView === 'graph') {
        router.push({ view: 'graph', file: filePath, typeFilter: typeName })
        return
      }
      router.push({ view: 'table', file: filePath, typeFilter: typeName })
    },
    [project?.file_types, router]
  )

  const openRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate) => {
      setPreferredView('record')
      const id = workspaceTabId(filePath, coordinate.actual_type)
      setWorkspaceTabs(current => current.some(tab => tab.id === id)
        ? current
        : [...current, { id, filePath, typeName: coordinate.actual_type }])
      setActiveWorkspaceTabId(id)
      setActiveType(coordinate.actual_type)
      router.push({ view: 'record', file: filePath, coordinate })
    },
    [router]
  )

  // Click on a corner badge (on a record or field): reveal the first
  // matching diagnostic in the bottom panel. Falls back to record-level
  // (fieldPath = null) if there's no exact field-level match.
  const focusDiagnosticForAnchor = useCallback(
    (
      filePath: string,
      recordKeyValue: string,
      actualType: string | null,
      fieldPath: string | null,
    ) => {
      if (!project) return
      const source = project.diagnostics
      // Prefer field-level; only fall back to record-level when the caller
      // asked for a field. When they asked for the whole record we take the
      // first diagnostic on it regardless of field.
      let hit = fieldPath
        ? source.find(d => diagnosticMatchesAnchor(d, filePath, recordKeyValue, actualType, fieldPath))
        : undefined
      if (!hit) {
        hit = source.find(d => diagnosticMatchesAnchor(d, filePath, recordKeyValue, actualType, null))
      }
      if (!hit) return
      setDiagFocus(prev => ({
        key: diagnosticKey(hit!),
        tick: (prev?.tick ?? 0) + 1,
      }))
    },
    [project],
  )

  const openRecordByKey = useCallback(
    (filePath: string, recordKey: string, actualType?: string | null) => {
      const cached = fileDataCache[filePath]
      const cachedRow = cached?.records.find(r =>
        r.coordinate.key === recordKey && (!actualType || r.coordinate.actual_type === actualType)
      )
      if (cachedRow) {
        openRecord(filePath, cachedRow.coordinate)
        return
      }
      if (!project || !api.isTauri) {
        setErrorMsg(`记录 ${recordKey} 未找到`)
        return
      }
      const sessionId = project.session_id
      const revision = project.revision
      const request = generation.captureRequest()
      api.getFileRecords(sessionId, filePath)
        .then(records => {
          if (!generation.isCurrent(sessionId, revision) || records.revision !== revision) return
          setFileDataCache(c => ({ ...c, [filePath]: records }))
          const row = records.records.find(r =>
            r.coordinate.key === recordKey && (!actualType || r.coordinate.actual_type === actualType)
          )
          if (row) openRecord(filePath, row.coordinate)
          else setErrorMsg(`记录 ${recordKey} 未找到`)
        })
        .catch(err => {
          if (generation.isRequestCurrent(request)) {
            reportSessionError(sessionId, '读取文件失败', err)
          }
        })
    },
    [fileDataCache, generation, openRecord, project, reportSessionError],
  )

  const rebindCoordinate = useCallback(
    (filePath: string, oldCoordinate: RecordCoordinate, newCoordinate: RecordCoordinate) => {
      if (sameCoordinate(oldCoordinate, newCoordinate)) return
      if (
        router.current?.view === 'record' &&
        router.current.file === filePath &&
        sameCoordinate(router.current.coordinate, oldCoordinate)
      ) {
        router.replace({ ...router.current, coordinate: newCoordinate })
      }
      setInspectorSelection(current => rebindSelection(
        current,
        filePath,
        oldCoordinate,
        newCoordinate,
      ))
    },
    [router],
  )
  const removeCoordinate = useCallback((filePath: string, coordinate: RecordCoordinate) => {
    setInspectorSelection(current => removeSelection(current, filePath, coordinate))
  }, [])

  const fileRecordsForRow = useCallback(
    (
      filePath: string,
      previousCoordinate: RecordCoordinate,
      row: RecordRow,
      revision: number,
    ): FileRecords | undefined => {
      const current = fileDataCacheRef.current[filePath]
      if (!current || current.revision !== revision - 1) return undefined
      let found = false
      const records = current.records.map(existing => {
        if (!sameCoordinate(existing.coordinate, previousCoordinate)) return existing
        found = true
        return row
      })
      return found ? { ...current, revision, records } : undefined
    },
    [],
  )

  const optimisticWriteField = useCallback((
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => {
    let appliedIdentity = generation.currentIdentity()
    let oldValue: FieldValue | undefined
    const optimisticValue = cloneValue(newValue)
    const apply = () => {
      const identity = generation.currentIdentity()
      const current = fileDataCacheRef.current[filePath]
      if (!identity || !current) return { changed: true }
      const projection = projectFieldValueAtRevision(
        current,
        identity.revision,
        coordinate,
        fieldPath,
        optimisticValue,
      )
      if (!projection) return { changed: true }
      if (!projection.changed) {
        if (appliedIdentity?.sessionId === identity.sessionId) appliedIdentity = identity
        return { changed: false, row: projection.row }
      }
      if (!projection.row || !projection.oldValue) return { changed: true }
      appliedIdentity = identity
      oldValue = projection.oldValue
      const projectedCache = { ...fileDataCacheRef.current, [filePath]: projection.records }
      fileDataCacheRef.current = projectedCache
      setFileDataCache(projectedCache)
      const projectedGraphs = projectGraphRows(
        graphCacheRef.current,
        current.revision,
        [projection.row],
      )
      graphCacheRef.current = projectedGraphs
      setGraphCache(projectedGraphs)
      return { changed: true, row: projection.row }
    }
    const initial = apply()
    return {
      ...initial,
      reapply: () => { apply() },
      rollback: () => {
        if (
          !appliedIdentity
          || !oldValue
          || !generation.isCurrent(appliedIdentity.sessionId, appliedIdentity.revision)
        ) return
        const latest = fileDataCacheRef.current[filePath]
        if (!latest) return
        const stillOptimistic = projectFieldValue(latest, coordinate, fieldPath, optimisticValue)
        if (stillOptimistic.changed) return
        const rollback = projectFieldValue(latest, coordinate, fieldPath, oldValue)
        if (!rollback.changed || !rollback.row) return
        const nextCache = { ...fileDataCacheRef.current, [filePath]: rollback.records }
        fileDataCacheRef.current = nextCache
        setFileDataCache(nextCache)
        const nextGraphs = projectGraphRows(
          graphCacheRef.current,
          latest.revision,
          [rollback.row],
        )
        graphCacheRef.current = nextGraphs
        setGraphCache(nextGraphs)
        appliedIdentity = null
      },
    }
  }, [generation])

  const mutationPort = useMemo<EditorMutationPort>(() => ({
    currentGeneration: () => api.isTauri ? generation.currentIdentity() : null,
    publish: publishMutation,
    fileRecordsForRow,
    rebindCoordinate,
    removeCoordinate,
    recoverPublication: (request, error) => {
      if (!commitProjectRevision(request.sessionId, request.revision, request.diagnostics)) {
        return false
      }
      window.setTimeout(() => {
        publishMutation(request).catch(retryError => {
          reportSessionError(
            request.sessionId,
            '后台刷新仍然失败',
            retryError ?? error,
            true,
            request.revision,
          )
        })
      }, 250)
      return true
    },
    reportError: (sessionId, prefix, error, expectedRevision) => {
      reportSessionError(sessionId, prefix, error, true, expectedRevision)
    },
    optimisticWriteField,
  }), [commitProjectRevision, fileRecordsForRow, generation, optimisticWriteField, publishMutation, rebindCoordinate, removeCoordinate, reportSessionError])
  const mutations = useMemo(
    () => new EditorMutationController(api, mutationPort, history),
    [history, mutationPort],
  )

  const writeDimensionCell = useCallback(async (
    data: api.DimensionFileRecords,
    row: api.DimensionFileRow,
    variant: string,
    expected: DimensionValueState,
    next: DimensionValueState,
  ) => {
    const coordinate: DimensionValueCoordinate = {
      actual_type: row.coordinate.actual_type,
      record_key: row.coordinate.key,
      field: data.field,
      dimension: data.dimension,
      variant,
      path: [],
    }
    const updateCache = (value: DimensionValueState, revision: number) => {
      setDimensionFileCache(cache => {
        const current = cache[data.file_path]
        if (!current) return cache
        return {
          ...cache,
          [data.file_path]: {
            ...current,
            revision,
            rows: current.rows.map(currentRow => sameCoordinate(currentRow.coordinate, row.coordinate)
              ? { ...currentRow, values: { ...currentRow.values, [variant]: value } }
              : currentRow),
          },
        }
      })
    }
    if (!api.isTauri) {
      updateCache(next, data.revision)
      return
    }
    const result = await mutations.writeDimensionValue(
      data.file_path,
      coordinate,
      expected,
      next,
    )
    if (!result) return
    updateCache(result, generation.currentIdentity()?.revision ?? data.revision)
  }, [generation, mutations])

  // Sidebar splitter: on mousedown, attach mousemove/mouseup listeners that
  // track the pointer X and clamp the new width to [160, 480]. Persist on
  // release. We use window listeners (not React state per move) so the drag
  // is smooth and doesn't re-render the whole tree on each pixel.
  const onSplitterMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setSplitterDragging(true)
    const startX = e.clientX
    const startW = sidebarW
    const onMove = (ev: MouseEvent) => {
      const next = Math.min(480, Math.max(160, startW + (ev.clientX - startX)))
      setSidebarW(next)
      document.documentElement.style.setProperty('--sidebar-w', `${next}px`)
    }
    const onUp = () => {
      setSplitterDragging(false)
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      try { localStorage.setItem('cfd-editor-sidebar-w', String(sidebarW)) } catch { /* quota */ }
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [sidebarW])

  // Apply the persisted width on mount and keep it in sync with keyboard
  // adjustments (the mouse-drag path sets the CSS var directly for speed).
  useEffect(() => {
    document.documentElement.style.setProperty('--sidebar-w', `${sidebarW}px`)
  }, [sidebarW])

  const writeField = useCallback(
    async (filePath: string, coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => {
      return mutations.writeField(filePath, coordinate, fieldPath, newValue)
    },
    [mutations],
  )

  const editCollection = useCallback(
    async (
      filePath: string,
      coordinate: RecordCoordinate,
      fieldPath: FieldPathSegment[],
      edit: import('./bindings/CollectionEdit').CollectionEdit,
    ) => {
      return mutations.editCollection(filePath, coordinate, fieldPath, edit)
    },
    [mutations],
  )

  const renameRecord = useCallback(
    async (filePath: string, coordinate: RecordCoordinate, newKey: string) => {
      const groups = projectSettings?.record_groups[filePath]?.[coordinate.actual_type] ?? []
      const result = await mutations.renameRecord(filePath, coordinate, newKey)
      if (result) {
        saveRecordGroups(
          filePath,
          coordinate.actual_type,
          replaceGroupedCoordinate(groups, coordinate, result.coordinate),
        )
      }
      return result
    },
    [mutations, projectSettings, saveRecordGroups],
  )

  const insertRecord = useCallback(
    async (filePath: string, recordKey: string, actualType: string, fields: FieldValue) => {
      await mutations.insertRecord(filePath, recordKey, actualType, fields)
    },
    [mutations],
  )
  const deleteRecord = useCallback(
    async (filePath: string, coordinate: RecordCoordinate) => {
      const groups = projectSettings?.record_groups[filePath]?.[coordinate.actual_type] ?? []
      await mutations.deleteRecord(filePath, coordinate)
      saveRecordGroups(
        filePath,
        coordinate.actual_type,
        removeRecordFromGroups(groups, coordinate),
      )
    },
    [mutations, projectSettings, saveRecordGroups],
  )
  const swapRecords = useCallback(
    async (filePath: string, first: RecordCoordinate, second: RecordCoordinate) => {
      await mutations.swapRecords(filePath, first, second)
    },
    [mutations],
  )
  const moveRecord = useCallback(
    async (filePath: string, coordinate: RecordCoordinate, targetIndex: number) => {
      await mutations.moveRecord(filePath, coordinate, targetIndex)
    },
    [mutations],
  )
  const transferRecord = useCallback(
    async (
      sourceFile: string,
      destinationFile: string,
      coordinate: RecordCoordinate,
      targetIndex: number,
    ) => {
      await mutations.transferRecord(sourceFile, destinationFile, coordinate, targetIndex)
    },
    [mutations],
  )

  const undo = useCallback(async () => {
    await mutations.undo()
  }, [mutations])

  const redo = useCallback(async () => {
    await mutations.redo()
  }, [mutations])

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault()
      }
      const historyShortcut = historyShortcutFor(e)
      if (historyShortcut) {
        e.preventDefault()
        if (historyShortcut === 'redo') redo()
        else undo()
      }
      if (e.altKey && e.key === 'ArrowLeft') router.back()
      if (e.altKey && e.key === 'ArrowRight') router.forward()
      // Ctrl+F / Cmd+F focuses the record search bar.
      if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
        e.preventDefault()
        globalSearchRef.current?.focus()
      }
      // `?` only toggles help when not focused inside a text-editing control,
      // otherwise typing `?` into inputs/search boxes would steal focus.
      if (e.key === '?' && !isTextTarget(e.target)) setShowHelp(v => !v)
      if (e.key === 'Escape') setShowHelp(false)
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [router, undo, redo])

  const currentRoute = router.current
  const activeFile = currentRoute?.file ?? null
  useEffect(() => setDimensionView('table'), [activeFile])
  useEffect(() => {
    if (!currentRoute) return
    const typeName = currentRoute.view === 'record'
      ? currentRoute.coordinate.actual_type
      : currentRoute.typeFilter ?? ''
    const id = workspaceTabId(currentRoute.file, typeName)
    if (!workspaceTabs.some(tab => tab.id === id)) return
    setActiveWorkspaceTabId(id)
    if (typeName) setActiveType(typeName)
  }, [currentRoute, workspaceTabs])
  const activeFileData = activeFile ? fileDataCache[activeFile] : null
  const activeDimensionData = activeFile ? dimensionFileCache[activeFile] : null
  const recordGroups = projectSettings?.record_groups[activeFile ?? '']?.[activeType] ?? []

  useEffect(() => {
    setInspectorSelection(current => {
      if (!current) return current
      if (!activeFileData || current.filePath !== activeFileData.file_path) return null
      const inActiveType = (coordinate: RecordCoordinate) => activeFileData.records.some(record => (
        sameCoordinate(record.coordinate, coordinate)
        && (!activeType || recordActualType(record) === activeType)
      ))
      if (current.kind === 'value') return inActiveType(current.coordinate) ? current : null
      const coordinates = current.coordinates.filter(inActiveType)
      if (coordinates.length === 0) return null
      return {
        ...current,
        coordinates,
        coordinate: coordinates.some(item => sameCoordinate(item, current.coordinate))
          ? current.coordinate
          : coordinates[coordinates.length - 1],
        anchor: coordinates.some(item => sameCoordinate(item, current.anchor))
          ? current.anchor
          : coordinates[0],
      }
    })
  }, [activeFileData?.file_path, activeType])

  useEffect(() => {
    setCollapsedRecordGroups(new Set())
  }, [activeFileData?.file_path, activeType])

  const toggleRecordGroup = useCallback((groupKey: string) => {
    setCollapsedRecordGroups(current => {
      const next = new Set(current)
      if (next.has(groupKey)) next.delete(groupKey)
      else next.add(groupKey)
      return next
    })
  }, [])
  const selectRecords = useCallback((
    file: string,
    coordinate: RecordCoordinate,
    visibleCoordinates: readonly RecordCoordinate[],
    mode: RecordSelectionMode,
  ) => {
    setInspectorCollapsed(false)
    setInspectorSelection(current => updateRecordSelection(
      current,
      file,
      coordinate,
      visibleCoordinates,
      mode,
    ))
  }, [])
  const dropRecordOntoRecord = useCallback((sources: readonly RecordCoordinate[], target: RecordCoordinate) => {
    if (!activeFile || !activeType) return
    recordGroupIdSequence.current += 1
    saveRecordGroups(
      activeFile,
      activeType,
      moveRecordsOntoRecord(
        recordGroups,
        sources,
        target,
        `record-group-${Date.now().toString(36)}-${recordGroupIdSequence.current.toString(36)}`,
        nextRecordGroupName(recordGroups),
      ),
    )
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const dropRecordsAfterRecord = useCallback(async (
    sources: readonly RecordCoordinate[], target: RecordCoordinate,
  ) => {
    if (!activeFile || !activeFileData) return
    const sourceIds = new Set(sources
      .filter(source => source.actual_type === target.actual_type)
      .map(coordinateId))
    if (sourceIds.has(coordinateId(target))) return
    const current = activeFileData.records
      .filter(row => row.coordinate.actual_type === target.actual_type)
      .sort((left, right) => left.container_index - right.container_index)
    const moving = current.filter(row => sourceIds.has(coordinateId(row.coordinate)))
    const remaining = current.filter(row => !sourceIds.has(coordinateId(row.coordinate)))
    const targetIndex = remaining.findIndex(row => sameCoordinate(row.coordinate, target))
    if (moving.length === 0 || targetIndex < 0) return
    const desired = [
      ...remaining.slice(0, targetIndex + 1),
      ...moving,
      ...remaining.slice(targetIndex + 1),
    ]
    for (let index = 0; index < desired.length; index += 1) {
      const currentIndex = current.findIndex(row => sameCoordinate(row.coordinate, desired[index].coordinate))
      if (currentIndex === index) continue
      await moveRecord(activeFile, desired[index].coordinate, index)
      const [moved] = current.splice(currentIndex, 1)
      current.splice(index, 0, moved)
    }
  }, [activeFile, activeFileData, moveRecord])
  const createManualRecordGroup = useCallback((records: readonly RecordCoordinate[]) => {
    if (!activeFile || !activeType) return
    recordGroupIdSequence.current += 1
    saveRecordGroups(
      activeFile,
      activeType,
      createRecordGroup(
        recordGroups,
        records,
        `record-group-${Date.now().toString(36)}-${recordGroupIdSequence.current.toString(36)}`,
        nextRecordGroupName(recordGroups),
      ),
    )
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const dropRecordIntoGroup = useCallback((sources: readonly RecordCoordinate[], groupId: string) => {
    if (!activeFile || !activeType) return
    saveRecordGroups(activeFile, activeType, moveRecordsToGroup(recordGroups, sources, groupId))
    setCollapsedRecordGroups(current => {
      if (!current.has(groupId)) return current
      const next = new Set(current)
      next.delete(groupId)
      return next
    })
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const dropRecordIntoUngrouped = useCallback((sources: readonly RecordCoordinate[]) => {
    if (!activeFile || !activeType) return
    saveRecordGroups(activeFile, activeType, removeRecordsFromGroups(recordGroups, sources))
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const renameManualRecordGroup = useCallback((groupId: string, name: string) => {
    if (!activeFile || !activeType) return
    saveRecordGroups(activeFile, activeType, renameRecordGroup(recordGroups, groupId, name))
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const colorManualRecordGroup = useCallback((groupId: string, color: string | null) => {
    if (!activeFile || !activeType) return
    saveRecordGroups(activeFile, activeType, colorRecordGroup(recordGroups, groupId, color))
  }, [activeFile, activeType, recordGroups, saveRecordGroups])
  const renameDimensionRecordGroup = useCallback((filePath: string, actualType: string, groupId: string, name: string) => {
    const groups = projectSettings?.record_groups[filePath]?.[actualType] ?? []
    saveRecordGroups(filePath, actualType, renameRecordGroup(groups, groupId, name))
  }, [projectSettings, saveRecordGroups])
  const colorDimensionRecordGroup = useCallback((filePath: string, actualType: string, groupId: string, color: string | null) => {
    const groups = projectSettings?.record_groups[filePath]?.[actualType] ?? []
    saveRecordGroups(filePath, actualType, colorRecordGroup(groups, groupId, color))
  }, [projectSettings, saveRecordGroups])
  const activeGraphKey = activeFile
    ? graphCacheKey(activeFile, GRAPH_DEPTH, GRAPH_LIMIT)
    : null
  const activeGraph = activeGraphKey ? graphCache[activeGraphKey] : null
  const readOnly = !isEditableFile(activeFileData)
  const fileCapabilities = useMemo(() => {
    const map: Record<string, WriterCapabilities> = {}
    for (const [file, records] of Object.entries(fileDataCache)) {
      map[file] = records.capabilities
    }
    return map
  }, [fileDataCache])
  const navigationFileTypes = useMemo(() => {
    if (!project) return {}
    const next = { ...project.file_types }
    for (const [filePath, records] of Object.entries(fileDataCache)) {
      const counts = new Map<string, number>()
      for (const record of records.records) {
        counts.set(record.coordinate.actual_type, (counts.get(record.coordinate.actual_type) ?? 0) + 1)
      }
      next[filePath] = (next[filePath] ?? []).map(option => ({
        ...option,
        record_count: counts.get(option.name) ?? 0,
      }))
    }
    return next
  }, [fileDataCache, project])
  const fileDiagnostics = useMemo(
    () => activeFile && project ? project.diagnostics.filter(d => d.file_path === activeFile) : [],
    [activeFile, project?.diagnostics],
  )
  // Prefer schema annotations, but also inspect values because older sessions
  // and browser mocks may contain refs without derived annotation metadata.
  const graphSupported = useMemo(() => {
    if (!activeFileData) return false
    return recordsSupportGraph(activeFileData.records)
  }, [activeFileData])
  // Set of file paths that can be opened via the record/table views. Used by
  // the diagnostics panel to decide whether "跳转" is available for a row —
  // if the diagnostic's file isn't part of the source set, we hide the button
  // instead of taking the user somewhere that will just say "记录未找到".
  const sourceFileSet = useMemo(
    () => project ? new Set(collectSourceFiles(project)) : new Set<string>(),
    [project],
  )

  // Record counts shown next to the search bar across all views. `typeCount`
  // is the number of records of the active type in the current file;
  // `matchedCount` additionally applies the global search filter (matches
  // record key, field names, or field value summaries — the union of what
  // Table and Record views each filter on so the count stays honest for
  // both).
  const { typeCount, matchedCount } = useMemo(() => {
    if (!activeFileData) return { typeCount: 0, matchedCount: 0 }
    const inType = activeType
      ? activeFileData.records.filter(r => recordActualType(r) === activeType)
      : activeFileData.records
    if (!globalSearch.trim()) return { typeCount: inType.length, matchedCount: inType.length }
    const matched = inType.filter(record => recordMatchesSearch(record, globalSearch))
    return { typeCount: inType.length, matchedCount: matched.length }
  }, [activeFileData, activeType, globalSearch])

  // A hidden row must not remain keyboard-selected after search narrows the
  // active set. Close a hidden table inspector, and keep record mode on the
  // first record that is still visible.
  useEffect(() => {
    if (!activeFileData || !currentRoute || !globalSearch.trim()) return
    const visible = activeFileData.records.filter(record => (
      (!activeType || recordActualType(record) === activeType)
      && recordMatchesSearch(record, globalSearch)
    ))
    setInspectorSelection(current => {
      if (!current || current.filePath !== currentRoute.file) return current
      if (current.kind === 'value') {
        return visible.some(record => sameCoordinate(record.coordinate, current.coordinate))
          ? current
          : null
      }
      const coordinates = current.coordinates.filter(coordinate => (
        visible.some(record => sameCoordinate(record.coordinate, coordinate))
      ))
      if (coordinates.length === 0) return null
      return {
        ...current,
        coordinates,
        coordinate: coordinates.some(item => sameCoordinate(item, current.coordinate))
          ? current.coordinate
          : coordinates[coordinates.length - 1],
        anchor: coordinates.some(item => sameCoordinate(item, current.anchor))
          ? current.anchor
          : coordinates[0],
      }
    })
    if (
      currentRoute.view === 'record'
      && visible.length > 0
      && !visible.some(record => sameCoordinate(record.coordinate, currentRoute.coordinate))
    ) {
      router.replace({ view: 'record', file: currentRoute.file, coordinate: visible[0].coordinate })
    }
  }, [activeFileData, activeType, currentRoute, globalSearch, router])

  // Stable callbacks for TableView so React.memo can bail out on re-renders
  // caused by inspector panel state changes (collapsed, open, width).
  const tableOnSelectRecord = useCallback(
    (coordinate: RecordCoordinate, mode: RecordSelectionMode, visible: readonly RecordCoordinate[]) => {
      if (currentRoute?.view === 'table') selectRecords(currentRoute.file, coordinate, visible, mode)
    },
    [currentRoute?.view, currentRoute?.file, selectRecords],
  )
  const writeFields = useCallback(
    async (
      filePath: string,
      coordinates: readonly RecordCoordinate[],
      fieldPath: FieldPathSegment[],
      newValue: FieldValue,
    ) => {
      await mutations.writeFields(filePath, coordinates, fieldPath, newValue)
    },
    [mutations],
  )
  const tableOnSelectValue = useCallback(
    (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => {
      if (currentRoute?.view === 'table') openValueInspector(currentRoute.file, coordinate, fieldPath)
    },
    [currentRoute?.view, currentRoute?.file, openValueInspector],
  )

  const closeWorkspaceTab = useCallback((id: string) => {
    const index = workspaceTabs.findIndex(tab => tab.id === id)
    if (index < 0) return
    const remaining = workspaceTabs.filter(tab => tab.id !== id)
    setWorkspaceTabs(remaining)
    if (id !== activeWorkspaceTabId) return
    const next = remaining[Math.min(index, remaining.length - 1)]
    if (!next) {
      setActiveWorkspaceTabId(null)
      setActiveType('')
      closeInspector()
      router.clear()
      return
    }
    openFile(next.filePath, next.typeName)
  }, [activeWorkspaceTabId, closeInspector, openFile, router, workspaceTabs])

  const focusFileTree = useCallback(() => {
    const tree = sidebarRef.current?.querySelector<HTMLElement>('.file-tree')
    const target = tree?.querySelector<HTMLElement>('[role="treeitem"][aria-selected="true"]')
      ?? tree?.querySelector<HTMLElement>('[role="treeitem"]')
    target?.focus({ preventScroll: true })
  }, [])
  const focusGlobalSearch = useCallback(() => {
    globalSearchRef.current?.focus({ preventScroll: true })
    globalSearchRef.current?.select()
  }, [])
  const focusDocumentTabs = useCallback(() => {
    document.querySelector<HTMLElement>('.document-view-tabs .tab-btn.active')
      ?.focus({ preventScroll: true })
  }, [])
  const focusActiveView = useCallback(() => {
    const target = viewContainerRef.current?.querySelector<HTMLElement>(
      '.table-scroll, .rv-sidebar-item.selected, .rv-main, .graph-view-wrap',
    )
    target?.focus({ preventScroll: true })
  }, [])
  const focusInspector = useCallback(() => {
    setInspectorCollapsed(false)
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>('.inspector-panel:not(.collapsed) .inspector-body')
        ?.focus({ preventScroll: true })
    })
  }, [])
  const focusFirstRecord = useCallback(() => {
    setFirstRecordFocusRequest(request => request + 1)
  }, [])
  const consumeFirstRecordFocusRequest = useCallback((request: number) => {
    setFirstRecordFocusRequest(current => current === request ? 0 : current)
  }, [])

  const runBuild = useCallback(async () => {
    const identity = generation.currentIdentity()
    if (!identity || projectAction) return
    setProjectAction('build')
    setProjectActionNotice(null)
    setErrorMsg(null)
    try {
      const result = await api.buildProject(identity.sessionId)
      setProjectActionNotice({
        message: result.replace('Build completed:', '构建完成：'),
        tone: 'success',
      })
    } catch (error) {
      const message = `构建失败: ${errorMessage(error)}`
      setErrorMsg(message)
      setProjectActionNotice({ message, tone: 'error' })
    } finally {
      setProjectAction(null)
    }
  }, [generation, projectAction])

  useEffect(() => {
    if (!projectActionNotice) return
    const timer = window.setTimeout(() => setProjectActionNotice(null), 4000)
    return () => window.clearTimeout(timer)
  }, [projectActionNotice])

  const openSourceFile = useCallback(async (filePath: string) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    try {
      await api.openSourceFile(identity.sessionId, filePath)
    } catch (error) {
      setErrorMsg(`打开源文件失败: ${errorMessage(error)}`)
    }
  }, [generation])
  const tableOnEnterInspector = useCallback(() => {
    setInspectorCollapsed(false)
    setInspectorFocusRequest(request => request + 1)
  }, [])
  const inspectorOnExitKeyboardNavigation = useCallback(() => {
    if (currentRoute?.view === 'table') setTableFocusRequest(request => request + 1)
    else focusActiveView()
  }, [currentRoute?.view, focusActiveView])
  const tableColumnWidths = useMemo(() => (
    currentRoute?.view === 'table' && activeType
      ? definedColumnWidths(projectSettings?.table_column_widths[currentRoute.file]?.[activeType])
      : undefined
  ), [activeType, currentRoute?.file, currentRoute?.view, projectSettings])
  const tableOnColumnWidthsChange = useCallback((widths: Record<string, number>) => {
    if (!api.isTauri || currentRoute?.view !== 'table' || !activeType) return
    const identity = generation.currentIdentity()
    if (!identity) return
    api.setTableColumnWidths(identity.sessionId, currentRoute.file, activeType, widths)
      .then(settings => {
        if (generation.isCurrent(identity.sessionId, identity.revision)) setProjectSettings(settings)
      })
      .catch(err => {
        if (generation.isCurrent(identity.sessionId, identity.revision)) {
          setErrorMsg(`保存列宽失败: ${errorMessage(err)}`)
        }
      })
  }, [activeType, currentRoute?.file, currentRoute?.view, generation])
  const tableOnRenderCellText = useCallback(
    async (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => {
      const identity = generation.currentIdentity()
      if (!identity) throw new Error('当前项目会话不可用')
      return api.renderCellText(identity.sessionId, coordinate, fieldPath)
    },
    [generation],
  )
  const tableOnParseCellText = useCallback(
    async (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], text: string) => {
      const identity = generation.currentIdentity()
      if (!identity) throw new Error('当前项目会话不可用')
      return api.parseCellText(identity.sessionId, coordinate, fieldPath, text)
    },
    [generation],
  )
  const tableOnOpenRecord = useCallback(
    (coordinate: RecordCoordinate) => {
      if (currentRoute?.view === 'table') openRecord(currentRoute.file, coordinate)
    },
    [currentRoute?.view, currentRoute?.file, openRecord],
  )
  const tableOnWriteField = useCallback(
    (coordinate: RecordCoordinate, path: FieldPathSegment[], val: FieldValue): Promise<RecordRow | void> => {
      if (currentRoute?.view === 'table') return writeField(currentRoute.file, coordinate, path, val)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, writeField],
  )
  const tableOnRenameRecord = useCallback(
    (coordinate: RecordCoordinate, newKey: string): Promise<RecordRow | void> => {
      if (currentRoute?.view === 'table') return renameRecord(currentRoute.file, coordinate, newKey)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, renameRecord],
  )
  const tableOnInsertRecord = useCallback(
    (rk: string, type: string, fields: FieldValue): Promise<void> => {
      if (currentRoute?.view === 'table') return insertRecord(currentRoute.file, rk, type, fields)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, insertRecord],
  )
  const tableOnCreateRecordDraft = useCallback(
    async (actualType: string): Promise<CreateRecordDraft> => {
      const result = await lookups.createRecordDraft(actualType)
      if (result.ok) return result.value
      if (result.reason === 'failed') throw new Error(result.error ?? '创建记录草稿失败')
      throw new Error('编辑器 generation 已更新')
    },
    [lookups],
  )
  const tableOnDeleteRecord = useCallback(
    (coordinate: RecordCoordinate): Promise<void> => {
      if (currentRoute?.view === 'table') return deleteRecord(currentRoute.file, coordinate)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, deleteRecord],
  )
  const tableOnMoveRecord = useCallback(
    (coordinate: RecordCoordinate, targetIndex: number): Promise<void> => {
      if (currentRoute?.view === 'table') return moveRecord(currentRoute.file, coordinate, targetIndex)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, moveRecord],
  )
  const tableOnBadgeClick = useCallback(
    (coordinate: RecordCoordinate, fieldPath: string | null) => {
      if (currentRoute?.view !== 'table') return
      focusDiagnosticForAnchor(currentRoute.file, coordinate.key, coordinate.actual_type, fieldPath)
    },
    [currentRoute?.view, currentRoute?.file, focusDiagnosticForAnchor],
  )

  // Help overlay: focus trap + autofocus + restore focus on close.
  useEffect(() => {
    if (!showHelp) return
    helpReturnRef.current = document.activeElement as HTMLElement | null
    const box = helpBoxRef.current
    if (box) {
      const focusable = box.querySelector<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      )
      focusable?.focus()
    }
    const handler = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return
      if (!box) return
      const nodes = Array.from(
        box.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        ),
      ).filter(el => !el.hasAttribute('disabled'))
      if (nodes.length === 0) return
      const first = nodes[0]
      const last = nodes[nodes.length - 1]
      const active = document.activeElement as HTMLElement | null
      if (e.shiftKey) {
        if (active === first || !box.contains(active)) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (active === last || !box.contains(active)) {
          e.preventDefault()
          first.focus()
        }
      }
    }
    window.addEventListener('keydown', handler)
    return () => {
      window.removeEventListener('keydown', handler)
      const ret = helpReturnRef.current
      if (ret && typeof ret.focus === 'function') ret.focus()
      helpReturnRef.current = null
    }
  }, [showHelp])

  // Sync the active type from the document target, falling back to the
  // first type reported for the file when an older route has no filter.
  useEffect(() => {
    if (!activeFileData) return
    const routedType = currentRoute?.view === 'record'
      ? currentRoute.coordinate.actual_type
      : currentRoute?.typeFilter
    const nextType = routedType && activeFileData.type_names.includes(routedType)
      ? routedType
      : activeFileData.type_names[0] ?? ''
    if (nextType !== activeType) setActiveType(nextType)
  }, [activeFileData?.file_path, activeFileData?.type_names, currentRoute, activeType])

  function switchView(view: 'table' | 'record' | 'graph') {
    if (!currentRoute) return
    setPreferredView(view)
    if (view === 'table') {
      setFirstRecordFocusRequest(0)
      closeInspector()
    }
    if (view === 'record') {
      const firstCoordinate =
        (activeType
          ? activeFileData?.records.find(r => recordActualType(r) === activeType)
          : activeFileData?.records[0])?.coordinate
        ?? activeFileData?.records[0]?.coordinate
      if (!firstCoordinate) return
      router.replace({ view, file: currentRoute.file, coordinate: firstCoordinate })
    } else {
      router.replace({ view, file: currentRoute.file, typeFilter: activeType } as typeof currentRoute)
    }
  }

  // When the user opens a file while `preferredView` is 'record', the initial
  // route was pushed as 'table' (record view needs a coordinate). Upgrade it
  // as soon as file data resolves and there's at least one record. Same
  // dance when the user picks a different type tab while in record view —
  // jump to the first record of the new type instead of stranding them on
  // the previous coordinate.
  useEffect(() => {
    if (!currentRoute) return
    if (activeFileData?.file_path !== currentRoute.file) return
    if (currentRoute.view === 'table' && preferredView === 'record') {
      const firstCoord = activeFileData.records.find(
        record => !activeType || recordActualType(record) === activeType,
      )?.coordinate
      if (firstCoord) {
        router.replace({ view: 'record', file: currentRoute.file, coordinate: firstCoord })
      }
      return
    }
    if (currentRoute.view === 'record' && activeType) {
      if (currentRoute.coordinate.actual_type !== activeType) {
        const firstOfType = activeFileData.records.find(
          r => recordActualType(r) === activeType,
        )?.coordinate
        if (firstOfType) {
          router.replace({ view: 'record', file: currentRoute.file, coordinate: firstOfType })
        }
      }
    }
  }, [currentRoute, preferredView, activeType, activeFileData, router])

  // If the current file doesn't support graph view but a stale route asks for
  // it, drop back to the table so the empty state isn't shown.
  useEffect(() => {
    if (!currentRoute || currentRoute.view !== 'graph') return
    if (activeFileData?.file_path !== currentRoute.file) return
    if (graphSupported) return
    router.replace({ view: 'table', file: currentRoute.file, typeFilter: activeType })
  }, [currentRoute, activeFileData, graphSupported, activeType, router])

  return (
    <ObjectDraftHost lookups={lookups} generationKey={lookupGenerationKey}>
    <div className="app">
      <div
        className="topbar"
        role="toolbar"
        aria-label="编辑器工具栏"
        onKeyDown={event => onToolbarKeyDown(event, focusFileTree)}
      >
        <div className="topbar-left">
          <span className="app-title">CFD Editor</span>
          <button className="btn btn-outlined" onClick={openProject}>
            <Icon name="open" size={13} />
            <span className="btn-label">打开</span>
          </button>
          <button
            className="btn btn-outlined"
            onClick={newProject}
            title="选一个空目录创建新的 Coflow 工程（等价于 coflow init）"
          >
            <Icon name="plus" size={13} />
            <span className="btn-label">新建</span>
          </button>
          <span className="topbar-divider" />
          <button
            className="btn btn-icon"
            onClick={router.back}
            disabled={!router.canBack}
            title="后退 (Alt+←)"
            aria-label="后退"
          >
            <Icon name="arrow-left" size={14} />
          </button>
          <button
            className="btn btn-icon"
            onClick={router.forward}
            disabled={!router.canForward}
            title="前进 (Alt+→)"
            aria-label="前进"
          >
            <Icon name="arrow-right" size={14} />
          </button>
        </div>
        <div className="topbar-center">
          {currentRoute && (activeFileData || activeDimensionData) ? (
            <>
              {activeDimensionData ? (
                <div className="document-view-tabs" role="tablist" aria-label="视图">
                  {(['record', 'table'] as const).map(view => (
                    <button
                      key={view}
                      className={`tab-btn tab-view${dimensionView === view ? ' active' : ''}`}
                      role="tab"
                      aria-selected={dimensionView === view}
                      onClick={() => setDimensionView(view)}
                    >
                      <Icon name={view} size={13} aria-hidden />
                      {view === 'table' ? '表格' : '记录'}
                    </button>
                  ))}
                </div>
              ) : activeFileData && <div className="document-view-tabs" role="tablist" aria-label="视图">
                {((['record', 'table', 'graph'] as const).filter(v => v !== 'graph' || graphSupported)).map(v => (
                  <button
                    key={v}
                    className={`tab-btn tab-view${currentRoute.view === v ? ' active' : ''}`}
                    role="tab"
                    aria-selected={currentRoute.view === v}
                    data-tab-id={v}
                    onClick={() => switchView(v)}
                  >
                    <Icon name={v === 'table' ? 'table' : v === 'record' ? 'record' : 'graph'} size={13} aria-hidden />
                    {v === 'table' ? '表格' : v === 'record' ? '记录' : '图谱'}
                  </button>
                ))}
              </div>}
              <button
                className="btn btn-primary btn-icon btn-build"
                onClick={runBuild}
                disabled={!project || projectAction !== null}
                title="构建项目"
                aria-label="构建项目"
              >
                <Icon name={projectAction === 'build' ? 'refresh' : 'build'} size={15} className={projectAction === 'build' ? 'icon-spin' : undefined} />
              </button>
            </>
          ) : null}
        </div>
        <div className="topbar-right">
          {(historySnapshot.undo.length > 0 || historySnapshot.redo.length > 0) && (
            <span className="undo-badge" title={`可撤销 ${historySnapshot.undo.length} 步 / 可重做 ${historySnapshot.redo.length} 步 (Ctrl+Z / Ctrl+Y)`}>
              {historySnapshot.undo.length > 0 ? `可撤销 ${historySnapshot.undo.length}` : `可重做 ${historySnapshot.redo.length}`}
            </span>
          )}
        </div>
      </div>

      {errorMsg && (
        <div className="error-banner" role="alert">
          <Icon name="error" size={13} />
          {errorMsg}
          <button className="btn btn-icon" onClick={() => setErrorMsg(null)} aria-label="关闭错误提示">
            <Icon name="close" size={12} />
          </button>
        </div>
      )}

      <div className="main-layout">
        <nav className="activity-bar" role="toolbar" aria-label="活动栏">
          <button
            className={`activity-btn${activePane === 'files' ? ' active' : ''}`}
            title="文件"
            aria-label="文件"
            aria-pressed={activePane === 'files'}
            onClick={() => { setActivePane('files'); focusFileTree() }}
          >
            <Icon name="folder" size={20} />
          </button>
          <button
            className={`activity-btn${activePane === 'search' ? ' active' : ''}`}
            title="搜索记录 (Ctrl+F)"
            aria-label="搜索"
            aria-pressed={activePane === 'search'}
            onClick={() => { setActivePane('search'); requestAnimationFrame(focusGlobalSearch) }}
          >
            <Icon name="search" size={20} />
          </button>
          <button
            className={`activity-btn${activePane === 'extensions' ? ' active' : ''}`}
            title="扩展"
            aria-label="扩展"
            aria-pressed={activePane === 'extensions'}
            onClick={() => setActivePane('extensions')}
          >
            <Icon name="extensions" size={20} />
          </button>
          <button
            className={`activity-btn${activePane === 'ai' ? ' active' : ''}`}
            title="AI 助手"
            aria-label="AI 助手"
            aria-pressed={activePane === 'ai'}
            onClick={() => setActivePane('ai')}
          >
            <Icon name="sparkles" size={20} />
          </button>
          <div className="activity-bar-bottom" ref={settingsMenuRef}>
            <button
              className="activity-btn"
              title={theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'}
              aria-label={theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'}
              onClick={toggleTheme}
            >
              <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={20} />
            </button>
            <button
              className={`activity-btn${settingsOpen ? ' active' : ''}`}
              title="设置"
              aria-label="设置"
              aria-haspopup="true"
              aria-expanded={settingsOpen}
              onClick={() => setSettingsOpen(v => !v)}
            >
              <Icon name="settings" size={20} />
            </button>
            {settingsOpen && (
              <div className="settings-dropdown" role="menu">
                <button
                  className="settings-item"
                  role="menuitem"
                  onClick={() => { setShowHelp(true); setSettingsOpen(false) }}
                >
                  <Icon name="help" size={14} />
                  <span>键盘快捷键 / 帮助</span>
                </button>
              </div>
            )}
          </div>
        </nav>
        <div className="sidebar" ref={sidebarRef}>
          {activePane === 'files' && (
            <>
              {project ? (
                <FileTree
                  nodes={project.file_tree}
                  dimensions={projectDimensions}
                  fileTypes={navigationFileTypes}
                  selectedFile={activeFile}
                  selectedType={activeType}
                  onSelectFile={openFile}
                  onExitRight={focusFirstRecord}
                  onOpenSourceFile={openSourceFile}
                />
              ) : (
                <div className="sidebar-empty">
                  {api.isTauri ? '未打开项目' : '浏览器预览（Mock）'}
                </div>
              )}
            </>
          )}
          {activePane === 'search' && (
            <>
              <div className="sidebar-header"><span>搜索</span></div>
              <div className="pane-search-wrap">
                <label className="pane-search">
                  <Icon name="search" size={13} />
                  <input
                    placeholder="按 key / 字段值搜索…"
                    value={globalSearch}
                    onChange={e => setGlobalSearch(e.target.value)}
                    aria-label="跨文件搜索"
                  />
                </label>
                <div className="pane-search-hint">
                  当前搜索会同时应用到打开的记录视图。在文档内用 Ctrl+F 直接聚焦。
                </div>
              </div>
            </>
          )}
          {activePane === 'extensions' && (
            <div className="extensions-pane">
              <div className="sidebar-header extensions-header">
                <span>扩展</span>
                {api.isTauri && (
                  <button className="btn btn-icon" onClick={() => void loadPluginFromSettings()} disabled={pluginLoadBusy} title="从文件安装插件" aria-label="从文件安装插件">
                    <Icon name="plus" size={15} />
                  </button>
                )}
              </div>
              <div className="extensions-list">
                {pluginSettings.map(plugin => (
                  <article className="extension-item" key={plugin.id}>
                    <div className="extension-item-main">
                      <div className="extension-item-icon"><Icon name="extensions" size={16} /></div>
                      <div>
                        <strong>{plugin.name}</strong>
                        <small>{plugin.description || plugin.id}</small>
                        <em>{plugin.origin === 'local' ? '本地已安装' : '内置'}</em>
                      </div>
                    </div>
                    <div className="extension-item-actions">
                      <label className="extension-toggle">
                        <input type="checkbox" checked={plugin.enabled} onChange={event => setReadPluginEnabled(plugin.id, event.target.checked)} />
                        <span>{plugin.enabled ? '已启用' : '已禁用'}</span>
                      </label>
                      {plugin.origin === 'local' && (
                        <button className="btn btn-icon" title="卸载插件" aria-label={`卸载 ${plugin.name}`} onClick={() => void uninstallPluginFromSettings(plugin.id)}>
                          <Icon name="close" size={13} />
                        </button>
                      )}
                    </div>
                  </article>
                ))}
                {pluginLoadError && <div className="extensions-error">{pluginLoadError}</div>}
              </div>
            </div>
          )}
          {activePane === 'ai' && (
            <>
              <div className="sidebar-header ai-header">
                <span>
                  <Icon name="sparkles" size={12} className="ai-header-icon" />
                  AI 助手
                </span>
              </div>
              <div className="ai-pane">
                <div className="ai-pane-placeholder">
                  <Icon name="sparkles" size={22} />
                  <div className="title">让 AI 帮你编辑配置</div>
                  <div className="hint">选中记录后描述你想做的修改，或让它检查配置一致性。</div>
                </div>
                <div className="ai-pane-suggest">
                  <button type="button" disabled>为选中的记录补齐缺失字段</button>
                  <button type="button" disabled>找出所有存在诊断的记录</button>
                  <button type="button" disabled>解释当前字段的类型定义</button>
                </div>
                <div className="ai-pane-input">
                  <textarea
                    placeholder="AI 助手尚未接入。当前仅为界面占位。"
                    disabled
                  />
                  <button type="button" className="ai-send" disabled aria-label="发送">
                    <Icon name="arrow-right" size={13} />
                  </button>
                </div>
              </div>
            </>
          )}
          {activePane === 'files' && api.isTauri && <UpdateControl />}
        </div>

        <div
          className={`sidebar-splitter${splitterDragging ? ' dragging' : ''}`}
          onMouseDown={onSplitterMouseDown}
          role="separator"
          aria-orientation="vertical"
          aria-label="调整侧栏宽度"
          tabIndex={0}
          onKeyDown={e => {
            if (e.key === 'ArrowLeft') setSidebarW(w => Math.max(160, w - 16))
            if (e.key === 'ArrowRight') setSidebarW(w => Math.min(480, w + 16))
          }}
        />

        <div className="editor-column">
        <div className="content-area-wrap">
        <div className="content-area">
          {workspaceTabs.length > 0 && (
            <div className="document-tabs" role="tablist" aria-label="已打开内容">
              <div
                className="tab-scroll"
                ref={tabScrollRef}
                onWheel={event => {
                  if (event.deltaX !== 0) return
                  const el = tabScrollRef.current
                  if (!el || Math.abs(event.deltaY) < 1) return
                  event.preventDefault()
                  el.scrollLeft += event.deltaY
                }}
              >
                {workspaceTabs.map(tab => {
                  const fileName = tab.filePath.split('/').pop() ?? tab.filePath
                  const types = project?.file_types[tab.filePath] ?? []
                  const type = types.find(option => option.name === tab.typeName)
                  const label = type
                    ? `${fileName} / ${type.display_name}`
                    : fileName
                  return (
                    <div
                      key={tab.id}
                      className={`document-tab${tab.id === activeWorkspaceTabId ? ' active' : ''}`}
                      role="tab"
                      aria-selected={tab.id === activeWorkspaceTabId}
                      tabIndex={tab.id === activeWorkspaceTabId ? 0 : -1}
                      data-tab-id={tab.id}
                      onClick={() => openFile(tab.filePath, tab.typeName)}
                      onKeyDown={event => {
                        if (event.key === 'Delete') {
                          event.preventDefault()
                          closeWorkspaceTab(tab.id)
                          return
                        }
                        onTabListKeyDown(
                          event,
                          workspaceTabs.map(item => item.id),
                          id => {
                            const target = workspaceTabs.find(item => item.id === id)
                            if (target) openFile(target.filePath, target.typeName)
                          },
                        )
                      }}
                      title={type && type.display_name !== type.name
                        ? `${tab.filePath} / ${type.display_name} (${type.name})`
                        : `${tab.filePath}${tab.typeName ? ` / ${tab.typeName}` : ''}`}
                    >
                      <Icon name="file" size={12} className="document-tab-icon" aria-hidden />
                      <span className="document-tab-label">{label}</span>
                      {readOnly && tab.id === activeWorkspaceTabId && <Icon name="lock" size={10} className="document-tab-lock" aria-hidden />}
                      <button
                        type="button"
                        className="document-tab-close"
                        onClick={event => {
                          event.stopPropagation()
                          closeWorkspaceTab(tab.id)
                        }}
                        aria-label={`关闭 ${label}`}
                        title="关闭标签"
                      >
                        <Icon name="close" size={11} aria-hidden />
                      </button>
                    </div>
                  )
                })}
              </div>
              {tabsOverflow && (
                <div className="tab-overflow" ref={tabOverflowRef}>
                  <button
                    type="button"
                    className="tab-overflow-btn"
                    onClick={() => setTabOverflowOpen(v => !v)}
                    aria-label="所有已打开标签"
                    title="所有已打开标签"
                    aria-expanded={tabOverflowOpen}
                  >
                    <Icon name="chevron-down" size={13} />
                  </button>
                  {tabOverflowOpen && (
                    <div className="tab-overflow-menu" role="menu">
                      {workspaceTabs.map(tab => {
                        const fileName = tab.filePath.split('/').pop() ?? tab.filePath
                        const types = project?.file_types[tab.filePath] ?? []
                        const type = types.find(option => option.name === tab.typeName)
                        const label = type
                          ? `${fileName} / ${type.display_name}`
                          : fileName
                        return (
                          <button
                            key={tab.id}
                            type="button"
                            role="menuitem"
                            className={`tab-overflow-item${tab.id === activeWorkspaceTabId ? ' active' : ''}`}
                            onClick={() => {
                              openFile(tab.filePath, tab.typeName)
                              setTabOverflowOpen(false)
                              requestAnimationFrame(() => {
                                const el = tabScrollRef.current?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(tab.id)}"]`)
                                el?.scrollIntoView({ inline: 'center', block: 'nearest' })
                              })
                            }}
                          >
                            <Icon name="file" size={12} aria-hidden />
                            <span className="name">{label}</span>
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
          {currentRoute && activeDimensionData ? (
            <DimensionTableView
              data={activeDimensionData}
              mode={dimensionView}
              recordGroupsByFile={projectSettings?.record_groups}
              onRenameGroup={renameDimensionRecordGroup}
              onColorGroup={colorDimensionRecordGroup}
              onWrite={(row, variant, expected, next) =>
                writeDimensionCell(activeDimensionData, row, variant, expected, next)}
              onExitLeft={focusFileTree}
              onExitUp={focusDocumentTabs}
              focusRequest={firstRecordFocusRequest}
              onFocusRequestConsumed={consumeFirstRecordFocusRequest}
            />
          ) : currentRoute && activeFileData ? (
            <>
              {readOnly && (
                <div className="document-toolbar readonly-only">
                  <span className="document-readonly" title="该来源未提供可写能力">
                    <Icon name="lock" size={11} aria-hidden />
                    只读
                  </span>
                </div>
              )}

              {/* Record search bar — shared across all three views */}
              <div className="global-search-bar">
                <Icon name="search" size={13} className="global-search-icon" aria-hidden />
                <input
                  ref={globalSearchRef}
                  placeholder="搜索记录… (Ctrl+F)"
                  value={globalSearch}
                  onChange={e => setGlobalSearch(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'ArrowDown') {
                      e.preventDefault()
                      focusFirstRecord()
                    } else if (e.key === 'ArrowLeft' && e.currentTarget.selectionStart === 0) {
                      e.preventDefault()
                      focusFileTree()
                    }
                  }}
                  aria-label="搜索记录"
                  role="searchbox"
                />
                {globalSearch && (
                  <button className="rv-clear-search" onClick={() => setGlobalSearch('')} aria-label="清除搜索">
                    <Icon name="close" size={13} aria-hidden />
                  </button>
                )}
                <span
                  className="global-search-count"
                  title={globalSearch ? `匹配 ${matchedCount} 条 / 共 ${typeCount} 条` : `共 ${typeCount} 条`}
                >
                  {globalSearch && matchedCount !== typeCount ? `${matchedCount} / ${typeCount}` : typeCount} 条
                </span>
              </div>

              <div className="view-container" ref={viewContainerRef}>
                {currentRoute.view === 'table' && (
                  <TableView
                    data={activeFileData}
                    activeType={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    searchQuery={globalSearch}
                    recordGroups={recordGroups}
                    collapsedGroupKeys={collapsedRecordGroups}
                    onToggleGroup={toggleRecordGroup}
                    onDropRecordOntoRecord={dropRecordOntoRecord}
                    onDropRecordAfterRecord={dropRecordsAfterRecord}
                    onCreateGroup={createManualRecordGroup}
                    onDropRecordIntoGroup={dropRecordIntoGroup}
                    onDropRecordIntoUngrouped={dropRecordIntoUngrouped}
                    onRenameGroup={renameManualRecordGroup}
                    onColorGroup={colorManualRecordGroup}
                    selection={inspectorSelection?.filePath === currentRoute.file
                      ? inspectorSelection
                      : null}
                    onSelectRecord={tableOnSelectRecord}
                    onSelectValue={tableOnSelectValue}
                    onRenderCellText={tableOnRenderCellText}
                    onParseCellText={tableOnParseCellText}
                    onClearSelection={closeInspector}
                    onOpenRecord={tableOnOpenRecord}
                    onWriteField={tableOnWriteField}
                    onRenameRecord={tableOnRenameRecord}
                    onInsertRecord={tableOnInsertRecord}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDeleteRecord={tableOnDeleteRecord}
                    onMoveRecord={tableOnMoveRecord}
                    onDiagnosticBadgeClick={tableOnBadgeClick}
                    columnWidths={tableColumnWidths}
                    onColumnWidthsChange={tableOnColumnWidthsChange}
                    onEnterInspector={tableOnEnterInspector}
                    focusRequest={tableFocusRequest}
                    firstRecordFocusRequest={firstRecordFocusRequest}
                    onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                    onNavigationBoundary={direction => {
                      if (direction === 'ArrowLeft') focusFileTree()
                      else if (direction === 'ArrowUp') focusGlobalSearch()
                    }}
                  />
                )}
                {currentRoute.view === 'record' && (
                  globalSearch.trim() && matchedCount === 0 ? (
                    <div className="empty-hint">无匹配 "{globalSearch}" 的记录</div>
                  ) : <RecordView
                    data={activeFileData}
                    coordinate={currentRoute.coordinate}
                    typeFilter={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    recordSearch={globalSearch}
                    recordGroups={recordGroups}
                    collapsedGroupKeys={collapsedRecordGroups}
                    onToggleGroup={toggleRecordGroup}
                    onDropRecordOntoRecord={dropRecordOntoRecord}
                    onDropRecordAfterRecord={dropRecordsAfterRecord}
                    onDropRecordIntoGroup={dropRecordIntoGroup}
                    onDropRecordIntoUngrouped={dropRecordIntoUngrouped}
                    onRenameGroup={renameManualRecordGroup}
                    onColorGroup={colorManualRecordGroup}
                    highlightField={highlightField}
                    onHighlightConsumed={() => setHighlightField(null)}
                    onOpenRecord={coordinate => openRecord(currentRoute.file, coordinate)}
                    onSelectRecord={(coordinate, mode, visible) => (
                      selectRecords(currentRoute.file, coordinate, visible, mode)
                    )}
                    selection={inspectorSelection}
                    onSelectValue={(coordinate, path) => {
                      setInspectorSelection(valueSelection(currentRoute.file, coordinate, path))
                    }}
                    onRenderCellText={tableOnRenderCellText}
                    onParseCellText={tableOnParseCellText}
                    onWriteField={(coordinate, path, val) => writeField(currentRoute.file, coordinate, path, val)}
                    onWriteFields={(coordinates, path, val) => writeFields(currentRoute.file, coordinates, path, val)}
                    onCollectionEdit={(coordinate, path, edit) => editCollection(currentRoute.file, coordinate, path, edit)}
                    onRenameRecord={(coordinate, newKey) => renameRecord(currentRoute.file, coordinate, newKey)}
                    onInsertRecord={(rk, type, fields) => insertRecord(currentRoute.file, rk, type, fields)}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDiagnosticBadgeClick={(coordinate, fieldPath) =>
                      focusDiagnosticForAnchor(currentRoute.file, coordinate.key, coordinate.actual_type, fieldPath)
                    }
                    onExitLeft={focusFileTree}
                    onExitUp={focusGlobalSearch}
                    firstRecordFocusRequest={firstRecordFocusRequest}
                    onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                  />
                )}
                {currentRoute.view === 'graph' && (
                  activeGraph ? (
                    <GraphView
                      graphData={activeGraph}
                      activeType={activeType}
                      enabledFieldsOverride={projectSettings?.graph_enabled_fields[activeFile ?? '']?.[activeType]}
                      onEnabledFieldsChange={fields => {
                        if (activeFile && activeType) saveGraphEnabledFields(activeFile, activeType, fields)
                      }}
                      fileCapabilities={fileCapabilities}
                      diagnostics={project?.diagnostics}
                      onOpenRecord={(file, coordinate) => openRecord(file, coordinate)}
                      onSelectRecord={openInspector}
                      onClearSelection={closeInspector}
                      selectedCoordinate={inspectorCoord}
                      onWriteField={writeField}
                      onCollectionEdit={editCollection}
                      onDiagnosticBadgeClick={(file, coordinate, fieldPath) =>
                        focusDiagnosticForAnchor(file, coordinate.key, coordinate.actual_type, fieldPath)
                      }
                      onExitLeft={focusFileTree}
                      onExitUp={focusGlobalSearch}
                      onExitRight={focusInspector}
                      firstRecordFocusRequest={firstRecordFocusRequest}
                      onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                    />
                  ) : (
                    <div className="empty-hint">加载图谱中…</div>
                  )
                )}
              </div>
            </>
          ) : loadingFile ? (
            <div className="content-empty">
              <div className="content-empty-title">加载 {loadingFile} 中…</div>
            </div>
          ) : (
            <div className="content-empty">
              <Icon name="open" size={40} />
              <div className="content-empty-title">
                {project ? '请选择文件' : '请打开项目'}
              </div>
              {!project && (
                <div className="content-empty-actions">
                  <button className="btn btn-primary" onClick={openProject}>
                    <Icon name="open" size={13} />
                    打开项目
                  </button>
                  <button className="btn btn-outlined" onClick={newProject}>
                    <Icon name="plus" size={13} />
                    新建工程
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
        <InspectorPanel
          open={currentRoute?.view !== 'record'
            && (inspectorOpen || ((currentRoute?.view === 'table' || currentRoute?.view === 'graph') && !!activeFileData))}
          collapsed={inspectorCollapsed}
          onToggleCollapse={() => setInspectorCollapsed(v => !v)}
          data={inspectorCoord ? fileDataCache[inspectorCoord.file] ?? null : null}
          selection={inspectorSelection}
          readOnly={inspectorCoord ? !isEditableFile(fileDataCache[inspectorCoord.file]) : true}
          diagnostics={project?.diagnostics}
          width={inspectorW}
          onWidthChange={setInspectorW}
          onClose={closeInspector}
          onWriteField={writeField}
          onWriteFields={writeFields}
          onRenderCellText={(_filePath, coordinate, path) => tableOnRenderCellText(coordinate, path)}
          onParseCellText={(_filePath, coordinate, path, text) => tableOnParseCellText(coordinate, path, text)}
          onCollectionEdit={editCollection}
          onRenameRecord={renameRecord}
          onDiagnosticBadgeClick={(coordinate, fieldPath) => {
            if (!inspectorCoord) return
            focusDiagnosticForAnchor(inspectorCoord.file, coordinate.key, coordinate.actual_type, fieldPath)
          }}
          focusRequest={inspectorFocusRequest}
          onExitKeyboardNavigation={inspectorOnExitKeyboardNavigation}
        />
        </div>
        {project && (
          <DiagnosticsPanel
            diagnostics={project.diagnostics}
            focus={diagFocus}
            onFocusConsumed={() => setDiagFocus(null)}
            isJumpable={(file) => sourceFileSet.has(file)}
            onJumpToRecord={(file, key, actualType) => {
              setHighlightField(RECORD_HIGHLIGHT_SENTINEL)
              openRecordByKey(file, key, actualType)
            }}
            onJumpToField={(file, key, actualType, fieldPath) => {
              setHighlightField(fieldPath)
              openRecordByKey(file, key, actualType)
            }}
          />
        )}
        </div>
      </div>

      {projectActionNotice && (
        <div
          className={`project-action-toast project-action-toast-${projectActionNotice.tone}`}
          role={projectActionNotice.tone === 'error' ? 'alert' : 'status'}
        >
          <Icon name={projectActionNotice.tone === 'error' ? 'error' : 'check'} size={14} />
          <span>{projectActionNotice.message}</span>
        </div>
      )}

      {showHelp && (
        <div className="help-overlay" onClick={() => setShowHelp(false)}>
          <div
            className="help-box"
            ref={helpBoxRef}
            role="dialog"
            aria-modal="true"
            aria-label="键盘快捷键"
            onClick={e => e.stopPropagation()}
          >
            <h3>
              <Icon name="help" size={16} />
              键盘快捷键
            </h3>
            <table>
              <tbody>
                <tr><td>Alt+←</td><td>后退</td></tr>
                <tr><td>Alt+→</td><td>前进</td></tr>
                <tr><td>Ctrl+Z</td><td>撤销编辑</td></tr>
                <tr><td>Ctrl+Y / Ctrl+Shift+Z</td><td>重做编辑</td></tr>
                <tr><td>?</td><td>显示/隐藏帮助</td></tr>
                <tr><td>Esc</td><td>关闭弹窗</td></tr>
              </tbody>
            </table>
            <div className="help-actions">
              <button className="btn btn-outlined" onClick={() => setShowHelp(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
    </ObjectDraftHost>
  )
}

function readLastProjectPath(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT_STORAGE_KEY)
  } catch {
    return null
  }
}

function rememberLastProject(yamlPath: string) {
  try {
    localStorage.setItem(LAST_PROJECT_STORAGE_KEY, yamlPath)
  } catch {
    // The project still opens when WebView storage is unavailable.
  }
}

function projectYamlPath(directory: string): string {
  const trimmed = directory.replace(/[\\/]+$/, '')
  const separator = trimmed.includes('\\') ? '\\' : '/'
  return `${trimmed}${separator}coflow.yaml`
}

function definedColumnWidths(
  widths: { [column: string]: number | undefined } | undefined,
): Record<string, number> | undefined {
  if (!widths) return undefined
  return Object.fromEntries(
    Object.entries(widths).filter((entry): entry is [string, number] => entry[1] !== undefined),
  )
}

function collectSourceFiles(snapshot: ProjectSnapshot): string[] {
  const out: string[] = []
  function walk(n: ProjectSnapshot['file_tree'][number]) {
    if (!n.is_dir && n.in_sources) out.push(n.path)
    for (const c of n.children) walk(c)
  }
  for (const n of snapshot.file_tree) walk(n)
  return out
}

function dimensionForFile(dimensions: DimensionInfo[], filePath: string): DimensionInfo | undefined {
  const normalizedFile = filePath.replace(/\\/g, '/')
  return dimensions.find(dimension => {
    if (!dimension.out_dir) return false
    const directory = dimension.out_dir.replace(/\\/g, '/').replace(/\/+$/, '')
    return normalizedFile.startsWith(`${directory}/`)
  })
}

/** True when the user is currently focused inside a text-editing control.
 *  Used to gate global shortcuts (`?`, etc.) so they don't fire while typing. */
function isTextTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable
}

/** Roving-tabindex arrow-key navigation for a `role="tablist"` of string ids. */
function onTabListKeyDown(
  e: React.KeyboardEvent,
  tabs: string[],
  onSelect: (id: string) => void,
  boundaries: {
    onLeftBoundary?: () => void
    onUp?: () => void
    onDown?: () => void
  } = {},
) {
  if (e.key === 'ArrowUp' && boundaries.onUp) {
    e.preventDefault()
    boundaries.onUp()
    return
  }
  if (e.key === 'ArrowDown' && boundaries.onDown) {
    e.preventDefault()
    boundaries.onDown()
    return
  }
  if (e.key === 'Enter') {
    e.preventDefault()
    const id = (e.currentTarget as HTMLElement).dataset.tabId
    if (id) onSelect(id)
    return
  }
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Home' && e.key !== 'End') return
  const nodes = Array.from(
    e.currentTarget.closest('[role="tablist"]')?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [],
  )
  const i = nodes.indexOf(e.currentTarget as HTMLElement)
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    e.preventDefault()
    const dir = e.key === 'ArrowRight' ? 1 : -1
    const nextIndex = i + dir
    if (nextIndex < 0) {
      boundaries.onLeftBoundary?.()
      return
    }
    if (nextIndex >= nodes.length) return
    const next = nodes[nextIndex]
    next.focus()
    const id = next.dataset.tabId
    if (id) onSelect(id)
  } else if (e.key === 'Home') {
    e.preventDefault()
    nodes[0]?.focus()
    const id = nodes[0]?.dataset.tabId
    if (id) onSelect(id)
  } else if (e.key === 'End') {
    e.preventDefault()
    const last = nodes[nodes.length - 1]
    last?.focus()
    const id = last?.dataset.tabId
    if (id) onSelect(id)
  }
}

function onToolbarKeyDown(event: React.KeyboardEvent, onExitDown: () => void) {
  if (!(event.target instanceof HTMLButtonElement)) return
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    onExitDown()
    return
  }
  if (
    event.key !== 'ArrowLeft'
    && event.key !== 'ArrowRight'
    && event.key !== 'Home'
    && event.key !== 'End'
  ) return
  // The center view switch (记录/表格/图谱) is intentionally excluded from the
  // toolbar's arrow-key roving nav — it's a mouse/click affordance, not part of
  // the left-to-right keyboard chain.
  const buttons = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled):not(.tab-view)'),
  )
  const index = buttons.indexOf(event.target)
  if (index < 0) return
  event.preventDefault()
  if (event.key === 'Home') buttons[0]?.focus()
  else if (event.key === 'End') buttons[buttons.length - 1]?.focus()
  else {
    const next = index + (event.key === 'ArrowRight' ? 1 : -1)
    if (next >= 0 && next < buttons.length) buttons[next].focus()
    else if (next < 0) onExitDown()
  }
}
