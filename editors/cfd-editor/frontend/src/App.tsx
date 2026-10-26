import { useContentMotion } from './hooks/useContentMotion'
import { useMutationFeedback } from './hooks/useMutationFeedback'
import { cellMotionOrigin, type MotionOrigin } from './utils/motion'
import type { RefTarget } from './bindings/RefTarget'
import { ReferencePanel } from './components/ReferencePanel'
import { ViewToolbar, ViewSearchField } from './components/ViewToolbar'
import { BuiltinViewPicker, ViewPicker } from './components/ViewPicker'
import { createMockLookups } from './mockLookups'
import { Menu } from './components/Menu'
import { ProjectSearchDialog } from './components/ProjectSearchDialog'
import { EditorSettingsMenu } from './components/EditorSettingsMenu'
import { recordMatchesKeySearch } from './value/fieldValue'
import { afterSourceSave, deferSourceNavigation as deferSourceNavigationOnly, flushSourceChanges } from './state/sourceAutosave'
import { lazy, Suspense, useState, useEffect, useCallback, useMemo, useRef, useSyncExternalStore } from 'react'
import { FileTree } from './components/FileTree'
import { CreateRecordDialog } from './components/CreateRecordDialog'
import { ConfirmDialog, TextInputDialog } from './components/ActionDialog'
import { ViewEditorDialog } from './components/ViewEditorDialog'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { InspectorPanel } from './components/InspectorPanel'
import { finishActiveDataEdit, deferDataNavigation } from './state/editSession'
import { Icon } from './components/Icon'
import { ObjectDraftHost } from './components/ObjectDraftHost'
import { ShortNameContext } from './components/ShortNameContext'
import { UpdateControl } from './components/UpdateControl'
import { HelpDialog } from './components/HelpDialog'
import { DocumentTabs, GIT_DIFF_TAB_ID, type PluginPageTab } from './components/DocumentTabs'
import { ActivityBar, type ActivePane } from './components/ActivityBar'
import { useRouter } from './hooks/useRouter'
import { useTheme } from './hooks/useTheme'
import { useFrontendPlugins } from './hooks/useFrontendPlugins'
import { useProjectDiff } from './hooks/useProjectDiff'
import { useEditorDataQueries } from './hooks/useEditorDataQueries'
import { useSidebarWidth } from './hooks/useSidebarWidth'
import { useProjectWatcher } from './hooks/useProjectWatcher'
import { useEditorProjections } from './hooks/useEditorProjections'
import { useWorkspacePersistence } from './hooks/useWorkspacePersistence'
import { useMutationPublication } from './hooks/useMutationPublication'
import { emptyProjectSettings, useProjectSettings } from './hooks/useProjectSettings'
import { searchMockRecords } from './built-in-plugins'
import {
  PluginContributionMount,
  dispatchPluginKeybinding,
  openPluginPage,
  pluginRegistrySnapshot,
  preferredPluginView,
  publishPluginEvent,
  setPluginDataBridge,
  setPluginUiBridge,
  usePluginRegistry,
  usePluginViews,
} from './plugins'
import type {
  PluginActiveContext,
  PluginMutationRequest,
  PluginPageContext,
  PluginRecordData,
  PluginSidebarContext,
  PluginViewContext,
} from './plugins/types'
import {
  MOCK_PROJECT,
  MOCK_FILE_RECORDS,
  MOCK_GRAPH,
  MOCK_EDITOR_SETTINGS,
} from './mock'
import * as api from './api'
import type { DimensionValueCoordinate } from './bindings/DimensionValueCoordinate'
import type { DimensionValueState } from './bindings/DimensionValueState'
import type { FileRecords } from './bindings/FileRecords'
import type { FlatDiagnostic } from './bindings/FlatDiagnostic'
import type { DiagnosticTarget } from './bindings/DiagnosticTarget'
import type { EditorProjectSettings } from './bindings/EditorProjectSettings'
import type { ViewConfig } from './bindings/ViewConfig'
import type { CreateRecordDraft } from './bindings/CreateRecordDraft'
import type { ProjectBootstrap } from './bindings/ProjectBootstrap'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import type { RecordRow } from './bindings/RecordRow'
import type { WriterCapabilities } from './bindings/WriterCapabilities'
import {
  diagnosticKey,
  diagnosticFilePath,
  diagnosticMatchesAnchor,
  errorMessage,
  cloneValue,
  isEditorError,
  recordActualType,
    coordinateId,
    sameCoordinate,
  type FieldPathSegment,
  type FieldValue,
} from './wire'
import { recordMatchesFullTextSearch } from './value/fieldValue'
import { isEditableFile } from './utils/editable'
import { isNativeEditorTarget } from './utils/dom'
import { EditorLookupController } from './state/editorLookups'
import {
  MutationHistoryController,
  ProjectGenerationController,
  committed, failed, superseded,
  type GraphPositions,
  type MutationResult,
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
  selectionMatchesValue,
  updateRecordSelection,
  updateValueSelection,
  valueSelection,
  RECORD_HIGHLIGHT_SENTINEL,
  selectedCellCount,
  type SelectedCells,
  type EditorSelection,
  type RecordSelectionMode,
  type ValueSelectionMode,
} from './state/editorSelection'
import type { BatchWriteFieldInput } from './bindings/BatchWriteFieldInput'
import {
  createRecordGroup,
  moveRecordsOntoRecord,
  moveRecordsToGroup,
  nextRecordGroupName,
  colorRecordGroup,
  removeRecordsFromGroups,
  renameRecordGroup,
  replaceGroupedCoordinate,
} from './state/manualRecordGroups'
import { graphSupportForFile, relationFieldNames } from './state/graphSupport'
import { reachableGraph } from './state/graphFilter'
import { queryClient } from './queryClient'
import { dimensionTargetId, dimensionViewPath, type DimensionTarget } from './state/dimensionNavigation'
import { editorQueryKeys } from './queryKeys'
import { fetchFileRecords, fetchSourceText, removeSessionQueries } from './editorQueries'
import {
  DEFAULT_RECORD_VIEW_ID,
  DEFAULT_SOURCE_VIEW_ID,
  DEFAULT_TABLE_VIEW_ID,
  viewTabsFor,
  resolveView,
  visibleFieldsFor,
  groupFilterPredicate,
  type ViewTab,
} from './state/views'
import {
  defaultWorkspaceTab,
  dimensionWorkspaceTab,
  routeForWorkspaceTab,
  sanitizeProjectWorkspace,
  workspaceToWire,
  workspaceTabId,
  workspaceTabWithView,
  type WorkspaceTab,
} from './state/workspaceTabs'
import coflowLogo from '../../../../assets/coflow-logo.svg'
import {
  collectSourceFiles,
  definedColumnWidths,
  dimensionForFile,
  graphCacheKey,
  graphViewKey,
  onToolbarKeyDown,
  projectGraphRows,
  projectYamlPath,
  readLastProjectPath,
  rememberLastProject,
  sameValueCells,
} from './state/appSupport'
import './style.css'

// 文件与视图导航共用草稿屏障，失败时保留原视图供用户修正。
function deferSourceNavigation(action: () => void): boolean {
  return deferDataNavigation(action) || deferSourceNavigationOnly(action)
}

// 预加载与 React.lazy 共享同一 Promise，切换视图时复用已经加载的模块。
function cachedViewModule<T>(loader: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | undefined
  return () => pending ??= loader()
}
const viewModules = {
  graph: cachedViewModule(() => import('./components/GraphView').then(module => ({ default: module.GraphView }))),
  source: cachedViewModule(() => import('./components/SourceEditorView').then(module => ({ default: module.SourceEditorView }))),
  table: cachedViewModule(() => import('./components/TableView').then(module => ({ default: module.TableView }))),
  record: cachedViewModule(() => import('./components/RecordView').then(module => ({ default: module.RecordView }))),
  dimension: cachedViewModule(() => import('./components/DimensionTableView').then(module => ({ default: module.DimensionTableView }))),
  diff: cachedViewModule(() => import('./components/GitDiffMode').then(module => ({ default: module.GitDiffMode }))),
}
function preloadView(kind: string) {
  if (kind in viewModules) void viewModules[kind as keyof typeof viewModules]().catch(() => { /* 实际视图接管加载失败。 */ })
}
const GraphView = lazy(viewModules.graph)
const SourceEditorView = lazy(viewModules.source)
const TableView = lazy(viewModules.table)
const RecordView = lazy(viewModules.record)
const DimensionTableView = lazy(viewModules.dimension)
const GitDiffMode = lazy(viewModules.diff)
const GitDiffSidebar = lazy(() => import('./components/GitDiffMode').then(module => ({ default: module.GitDiffSidebar })))

const ViewLoading = () => <div className="content-empty" role="status"><div className="content-empty-title">加载视图中…</div></div>

const GRAPH_DEPTH = 3
const GRAPH_LIMIT = 1_000
// 共享空数组，避免每次渲染产生新引用而让下游 useMemo 失效。
const EMPTY_RECORD_GROUPS: never[] = []
interface AppProps {
  browserSettings?: EditorProjectSettings
}

export default function App({ browserSettings = MOCK_EDITOR_SETTINGS }: AppProps = {}) {
  const [project, setProject] = useState<ProjectBootstrap | null>(null)
  useEffect(() => {
    if (!project) return
    // 三个基础视图并行预加载，不等待模块完成，项目界面和文件读取可以继续。
    for (const kind of ['record', 'table', 'source']) preloadView(kind)
  }, [project])
  const {
    settings: pluginSettings,
    ready: pluginsReady,
    busy: pluginLoadBusy,
    error: pluginLoadError,
    install: loadPluginFromSettings,
    uninstall: uninstallPluginFromSettings,
    toggle: togglePluginFromSettings,
  } = useFrontendPlugins(project)
  const pluginRegistry = usePluginRegistry()
  useEffect(() => {
    const suppressBrowserMenu = (event: MouseEvent) => event.preventDefault()
    window.addEventListener('contextmenu', suppressBrowserMenu)
    return () => window.removeEventListener('contextmenu', suppressBrowserMenu)
  }, [])
  const [generation] = useState(() => new ProjectGenerationController())
  const [history] = useState(() => new MutationHistoryController())
  const [lookups] = useState(() => new EditorLookupController(api.isTauri ? api : createMockLookups(() => fileDataCacheRef.current)))
  const lookupGenerationKey = project ? `${project.session_id}:${project.revision}` : 'none'
  const historySnapshot = useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot)
  const {
    files: fileDataCache,
    graphs: graphCache,
    filesRef: fileDataCacheRef,
    graphsRef: graphCacheRef,
    setFiles: setFileDataCache,
    setGraphs: setGraphCache,
    reset: resetProjections,
  } = useEditorProjections()
  const projectDimensions = project?.dimensions ?? []
  const [dimensionView, setDimensionView] = useState<'table' | 'record'>('table')
  const [showHelp, setShowHelp] = useState(false)
  const [treeRecordDraft, setTreeRecordDraft] = useState<{ filePath: string; actualType: string; data: FileRecords } | null>(null)
  const [fileActionDialog, setFileActionDialog] = useState<
    | { kind: 'create'; parentPath: string; sourceKind: 'schema' | 'data' }
    | { kind: 'delete'; path: string }
    | null
  >(null)
  const [fileActionBusy, setFileActionBusy] = useState(false)
  const [recoverySession, setRecoverySession] = useState<number | null>(null)
  const [referenceHistory, setReferenceHistory] = useState<RefTarget[]>([])
  const [referenceError, setReferenceError] = useState<string | null>(null)
  const referenceTarget = referenceHistory[referenceHistory.length - 1]
  useEffect(() => { setReferenceHistory([]) }, [project?.session_id])
  useEffect(() => {
    if (!referenceTarget || !project) return
    const identity = generation.currentIdentity()
    if (!identity) return
    const file = referenceTarget.file_path
    let cancelled = false
    setReferenceError(null)
    if (fileDataCacheRef.current[file]?.revision === identity.revision) return
    void fetchFileRecords(queryClient, identity.sessionId, identity.revision, file, () => api.getFileRecords(identity.sessionId, file)).then(data => {
      if (!cancelled && generation.isCurrent(identity.sessionId, identity.revision)) setFileDataCache(current => ({ ...current, [file]: data }))
    }).catch(error => { if (!cancelled) setReferenceError(errorMessage(error)) })
    return () => { cancelled = true }
  }, [referenceTarget, project?.revision, project?.session_id, generation, setFileDataCache])
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const {
    settings: projectSettings,
    setSettings: setProjectSettings,
    saveRecordGroups,
    saveViews,
    saveViewOrder,
  } = useProjectSettings(generation, setErrorMsg)
  const graphSettingsRef = useRef(projectSettings)
  graphSettingsRef.current = projectSettings

  const applyGraphPositions = useCallback(async (viewKey: string, positions: GraphPositions): Promise<MutationResult<void>> => {
    const identity = generation.currentIdentity()
    if (!identity) return superseded()
    try {
      if (api.isTauri) await api.setGraphPositions(identity.sessionId, viewKey, positions)
      if (generation.currentIdentity()?.sessionId !== identity.sessionId) return superseded()
      const base = graphSettingsRef.current ?? emptyProjectSettings()
      graphSettingsRef.current = { ...base, graph_positions: { ...base.graph_positions, [viewKey]: positions } }
      setProjectSettings(current => {
        const base = current ?? emptyProjectSettings()
        return { ...base, graph_positions: { ...base.graph_positions, [viewKey]: positions } }
      })
      return committed(undefined)
    } catch (error) {
      setErrorMsg(`保存图节点位置失败：${errorMessage(error)}`)
      return failed()
    }
  }, [generation, setProjectSettings])

  const saveGraphPositions = useCallback((viewKey: string, positions: GraphPositions, recordHistory: boolean) => {
    const identity = generation.currentIdentity()
    const epoch = history.currentEpoch()
    return history.serialize(async () => {
      if (!identity || generation.currentIdentity()?.sessionId !== identity.sessionId || history.currentEpoch() !== epoch) return
      const oldPositions = (graphSettingsRef.current?.graph_positions[viewKey] ?? {}) as GraphPositions
      const next = structuredClone(positions)
      const result = await applyGraphPositions(viewKey, next)
      if (result.status === 'failed') throw new Error('保存图节点位置失败')
      if (result.status === 'committed' && recordHistory) {
        history.record({ kind: 'graph-layout', viewKey,
          revision: generation.currentIdentity()!.revision,
          oldPositions: structuredClone(oldPositions), newPositions: next })
      }
    })
  }, [generation, history, applyGraphPositions])

  // 图视图缩略/完整模式是纯展示偏好，直接落盘，不进入撤销历史。
  const saveGraphCompactMode = useCallback(async (viewKey: string, compact: boolean) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    // 先乐观更新本地设置，按钮切换即时生效。
    const base = graphSettingsRef.current ?? emptyProjectSettings()
    graphSettingsRef.current = {
      ...base,
      graph_compact_modes: { ...base.graph_compact_modes, [viewKey]: compact },
    }
    setProjectSettings(current => {
      const base = current ?? emptyProjectSettings()
      return { ...base, graph_compact_modes: { ...base.graph_compact_modes, [viewKey]: compact } }
    })
    if (!api.isTauri) return
    try {
      await api.setGraphCompactMode(identity.sessionId, viewKey, compact)
    } catch (error) {
      if (generation.currentSession() === identity.sessionId) {
        setErrorMsg(`保存图视图模式失败：${errorMessage(error)}`)
      }
    }
  }, [generation, setProjectSettings])

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
  const [projectAction, setProjectAction] = useState<'build' | null>(null)
  const [buildPending, setBuildPending] = useState(false)

  useEffect(() => {
    const showNotice = (event: Event) => setErrorMsg((event as CustomEvent<string>).detail)
    window.addEventListener('cfd-editor-notice', showNotice)
    return () => window.removeEventListener('cfd-editor-notice', showNotice)
  }, [])
  const buildStatusRequestRef = useRef(0)
  const [projectActionNotice, setProjectActionNotice] = useState<{
    message: string
    tone: 'success' | 'error'
  } | null>(null)

  const router = useRouter()
  const dataQueries = useEditorDataQueries(
    project,
    projectDimensions,
    router.current,
    generation,
    GRAPH_DEPTH,
    GRAPH_LIMIT,
  )
  const { theme, toggle: toggleTheme } = useTheme()
  const [activeType, setActiveType] = useState<string>('')
  const [workspaceTabs, setWorkspaceTabs] = useState<WorkspaceTab[]>([])
  const [activeWorkspaceTabId, setActiveWorkspaceTabId] = useState<string | null>(null)
  const [pluginPageTabs, setPluginPageTabs] = useState<PluginPageTab[]>([])
  const [activePluginPageKey, setActivePluginPageKey] = useState<string | null>(null)
  const workspaceTabsRef = useRef(workspaceTabs)
  const pluginDefaultPendingTabsRef = useRef(new Set<string>())
  const [workspaceReadySessionId, setWorkspaceReadySessionId] = useState<number | null>(null)
  workspaceTabsRef.current = workspaceTabs
  const closePluginPageTab = useCallback((key: string) => {
    setPluginPageTabs(current => current.filter(tab => tab.key !== key))
    setActivePluginPageKey(current => current === key ? null : current)
  }, [])
  const openPluginPageTab = useCallback((pluginId: string, pageId: string): void => {
    if (deferSourceNavigation(() => openPluginPageTab(pluginId, pageId))) return
    const page = pluginRegistrySnapshot().pages.find(item => item.pluginId === pluginId && item.id === pageId)
    if (!page) {
      setErrorMsg(`插件页面 ${pluginId}/${pageId} 不可用`)
      return
    }
    finishActiveDataEdit()
    setGitDiffActive(false)
    setPluginPageTabs(current => current.some(tab => tab.key === page.key)
      ? current
      : [...current, { key: page.key, title: page.title }])
    setActivePluginPageKey(page.key)
  }, [])
  const [activePane, setActivePane] = useState<ActivePane>(() => {
    try {
      const v = localStorage.getItem('cfd-editor-active-pane')
      return v === 'changes' || v === 'plugins' || v?.startsWith('plugin:') ? v as ActivePane : 'files'
    } catch { return 'files' }
  })
  const {
    open: gitDiffOpen,
    setOpen: setGitDiffOpen,
    active: gitDiffActive,
    setActive: setGitDiffActive,
    diff: projectDiff,
    loading: projectDiffLoading,
    error: projectDiffError,
    selection: gitDiffSelection,
    setSelection: setGitDiffSelection,
    load: loadProjectDiff,
  } = useProjectDiff(project, generation, activePane === 'changes')
  const pluginOpenRecordRef = useRef((
    _filePath: string,
    _coordinate: RecordCoordinate,
    _fieldPath?: string | null,
  ) => {})
  const [projectSearchOpen, setProjectSearchOpen] = useState(false)
  const openPluginSidebar = useCallback((pluginId: string, sidebarId: string) => {
    const sidebar = pluginRegistrySnapshot().sidebars.find(
      item => item.pluginId === pluginId && item.id === sidebarId,
    )
    if (!sidebar) {
      setErrorMsg(`插件侧栏 ${pluginId}/${sidebarId} 不可用`)
      return
    }
    if (pluginId === 'project-search') {
      setProjectSearchOpen(true)
      return
    }
    setNavigationCollapsed(false)
    setActivePane(`plugin:${sidebar.key}`)
  }, [])
  useEffect(() => {
    setPluginUiBridge({
      openPage: openPluginPageTab,
      closePage: (pluginId, pageId) => {
        const prefix = `${pluginId}/`
        if (pageId) closePluginPageTab(`${prefix}${pageId}`)
        else {
          setPluginPageTabs(current => current.filter(tab => !tab.key.startsWith(prefix)))
          setActivePluginPageKey(current => current?.startsWith(prefix) ? null : current)
        }
      },
      openSidebar: openPluginSidebar,
      closeSidebar: (pluginId, sidebarId) => {
        const pane = `plugin:${pluginId}/${sidebarId}`
        setActivePane(current => current === pane ? 'files' : current)
      },
      openRecord: (filePath, coordinate, fieldPath) => {
        pluginOpenRecordRef.current(filePath, coordinate, fieldPath)
      },
      reportError: setErrorMsg,
    })
    return () => setPluginUiBridge(null)
  }, [closePluginPageTab, openPluginPageTab, openPluginSidebar])
  useEffect(() => {
    if (!pluginsReady) return
    const available = new Set(pluginRegistry.pages.map(page => page.key))
    setPluginPageTabs(current => current.filter(tab => available.has(tab.key)))
    setActivePluginPageKey(current => current && available.has(current) ? current : null)
    if (activePane.startsWith('plugin:') && !pluginRegistry.sidebars.some(sidebar => `plugin:${sidebar.key}` === activePane)) {
      setActivePane('files')
    }
  }, [activePane, pluginRegistry.pages, pluginRegistry.sidebars, pluginsReady])
  const projectSearchContribution = pluginRegistry.sidebars.find(sidebar => sidebar.pluginId === 'project-search')
  const projectSearchContext = useMemo<PluginSidebarContext | null>(() => projectSearchContribution ? {
    identity: project ? { sessionId: project.session_id, revision: project.revision } : null,
    sidebarId: projectSearchContribution.id,
    openPage: pageId => openPluginPageTab(projectSearchContribution.pluginId, pageId),
    openRecord: (filePath, coordinate, fieldPath) => {
      setProjectSearchOpen(false)
      pluginOpenRecordRef.current(filePath, coordinate, fieldPath)
    },
    closeSidebar: () => setProjectSearchOpen(false),
  } : null, [projectSearchContribution, project, openPluginPageTab])
  const activePluginSidebar = activePane.startsWith('plugin:')
    ? pluginRegistry.sidebars.find(sidebar => `plugin:${sidebar.key}` === activePane)
    : undefined
  const pluginSidebarContext = useMemo<PluginSidebarContext | null>(() => (
    activePluginSidebar
      ? {
          identity: project ? { sessionId: project.session_id, revision: project.revision } : null,
          sidebarId: activePluginSidebar.id,
          openPage: pageId => openPluginPageTab(activePluginSidebar.pluginId, pageId),
          openRecord: (filePath, coordinate, fieldPath) => {
            pluginOpenRecordRef.current(filePath, coordinate, fieldPath)
          },
          closeSidebar: () => setActivePane('files'),
        }
      : null
  ), [activePluginSidebar, openPluginPageTab, project])
  useEffect(() => {
    try { localStorage.setItem('cfd-editor-active-pane', activePane) } catch { /* quota */ }
  }, [activePane])
  const [documentSearch, setDocumentSearch] = useState('')
  const [sourceSearch, setSourceSearch] = useState({ file: '', query: '', step: 0 })
  const [sourceSearchStatus, setSourceSearchStatus] = useState({ file: '', current: 0, total: 0 })
  const [keyOnlySearch, setKeyOnlySearch] = useState(false)
  const [collapsedRecordGroups, setCollapsedRecordGroups] = useState<Set<string>>(() => new Set())
  const recordGroupIdSequence = useRef(0)
  const documentSearchRef = useRef<HTMLInputElement>(null)
  const sidebarRef = useRef<HTMLDivElement>(null)
  const viewContainerRef = useRef<HTMLDivElement>(null)
  const lookupDiagnostics = useSyncExternalStore(lookups.subscribe, lookups.getFailures, lookups.getFailures)
  const [navigationCollapsed, setNavigationCollapsed] = useState(false)
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
  const [sourceDiagnosticFocus, setSourceDiagnosticFocus] = useState<{
    file: string
    range: Extract<DiagnosticTarget, { kind: 'source' | 'project_source' }>['range']
    tick: number
  } | null>(null)

  // View editor dialog + view-tab context menu (create/edit/delete custom views).
  const [viewEditor, setViewEditor] = useState<
    { mode: 'create' } | { mode: 'edit'; view: ViewConfig } | null
  >(null)
  const openViewEditor = () => setViewEditor({ mode: 'create' })

  const {
    dragging: splitterDragging,
    resizeBy: resizeSidebar,
    onSplitterMouseDown,
  } = useSidebarWidth()

  // Right-side inspector panel: table cells select one value, while the Key
  // column and graph nodes select the whole record.
  const [inspectorDialogOpen, setInspectorDialogOpen] = useState(false)
  const [inspectorOrigin, setInspectorOrigin] = useState<MotionOrigin | null>(null)
  useMutationFeedback(historySnapshot, inspectorOrigin, inspectorDialogOpen)
  const [inspectorSelection, setInspectorSelection] = useState<EditorSelection | null>(null)
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
    finishActiveDataEdit()
    setInspectorSelection(prev => {
      if (prev?.kind === 'record' && prev.filePath === file && sameCoordinate(prev.coordinate, coordinate)) return prev
      return recordSelection(file, coordinate)
    })
  }, [])
  const closeInspector = useCallback(() => {
    setInspectorDialogOpen(false)
    setInspectorOrigin(null)
  }, [])

  const clearWorkspaceSelection = useCallback(() => {
    if (deferDataNavigation(clearWorkspaceSelection)) return
    setInspectorSelection(null)
    closeInspector()
  }, [closeInspector])

  const navigateWorkspaceTab = useCallback((
    tab: WorkspaceTab,
    coordinate?: RecordCoordinate,
  ): void => {
    if (deferSourceNavigation(() => navigateWorkspaceTab(tab, coordinate))) return
    preloadView(tab.dimensionTarget ? 'dimension' : tab.viewKind)
    finishActiveDataEdit()
    setGitDiffActive(false)
    setActivePluginPageKey(null)
    setActiveWorkspaceTabId(tab.id)
    setActiveType(tab.typeName)
    if (tab.viewKind === 'source' && api.isTauri) {
      const identity = generation.currentIdentity()
      if (identity) void fetchSourceText(queryClient, identity.sessionId, identity.revision, tab.filePath, () => api.readSourceText(identity.sessionId, tab.filePath)).catch(() => { /* 源码视图展示读取错误。 */ })
    }
    if (tab.filePath.endsWith('.cft')) {
      router.push({
        view: 'source',
        file: tab.filePath,
        viewId: DEFAULT_SOURCE_VIEW_ID,
        typeFilter: '',
      })
      return
    }
    if (tab.dimensionTarget || !tab.typeName) {
      if (tab.dimensionTarget) setDimensionView(tab.viewKind === 'record' ? 'record' : 'table')
      router.push(routeForWorkspaceTab(tab))
      return
    }
    const fallbackCoordinate = coordinate ?? fileDataCacheRef.current[tab.filePath]?.records.find(
      row => recordActualType(row) === tab.typeName,
    )?.coordinate
    router.push(routeForWorkspaceTab(tab, fallbackCoordinate))
  }, [generation, router])

  const installWorkspace = useCallback((
    bootstrap: ProjectBootstrap,
    settings: EditorProjectSettings,
  ) => {
    const sourceFiles = collectSourceFiles(bootstrap)
    const restored = sanitizeProjectWorkspace(
      settings.workspace,
      bootstrap.file_types,
      new Set(sourceFiles),
      bootstrap.dimensions,
    )
    let tabs = restored?.tabs ?? []
    let activeTabId = restored?.activeTabId ?? null
    if (tabs.length === 0) {
      const firstFile = bootstrap.first_source_file ?? sourceFiles[0]
      if (firstFile) {
        const option = bootstrap.file_types[firstFile]?.[0]
        const tab = defaultWorkspaceTab(firstFile, option?.name ?? '')
        tabs = [tab]
        activeTabId = tab.id
        pluginDefaultPendingTabsRef.current.add(tab.id)
      }
    }
    workspaceTabsRef.current = tabs
    setWorkspaceTabs(tabs)
    setActiveWorkspaceTabId(activeTabId)
    const active = tabs.find(tab => tab.id === activeTabId) ?? tabs[0]
    if (active) navigateWorkspaceTab(active)
    else {
      setActiveType('')
      router.clear()
    }
    setWorkspaceReadySessionId(bootstrap.session_id)
  }, [navigateWorkspaceTab, router])

  // Auto-load mock data only when not running in Tauri (browser preview).
  const mockProjectInitializedRef = useRef(false)
  useEffect(() => {
    if (!api.isTauri && !mockProjectInitializedRef.current) {
      mockProjectInitializedRef.current = true
      generation.adopt(MOCK_PROJECT)
      lookups.adopt({ sessionId: MOCK_PROJECT.session_id, revision: MOCK_PROJECT.revision })
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setProjectSettings(browserSettings)
      setGraphCache({ [graphCacheKey('data/npc.cfd', GRAPH_DEPTH, GRAPH_LIMIT)]: MOCK_GRAPH })
      installWorkspace(MOCK_PROJECT, browserSettings)
    }
  }, [generation, installWorkspace, lookups])

  // Reset all per-session UI state to a clean slate before swapping in a
  // new project snapshot. Used by both "open" and "new" flows so behavior
  // is identical. Also closes the previous backend session so the
  // SessionStore doesn't accumulate stale sessions across project switches.
  const adoptSnapshot = useCallback(
    (bootstrap: ProjectBootstrap) => {
      const previousSession = generation.adopt(bootstrap)
      if (previousSession !== null && previousSession !== bootstrap.session_id) {
        removeSessionQueries(queryClient, previousSession)
      }
      lookups.adopt({ sessionId: bootstrap.session_id, revision: bootstrap.revision })
      setProject(prev => {
        // Fire-and-forget close of the outgoing session. We read prev here
        // (not `project` from the closure) so we always close exactly the
        // session we're replacing, even if state was stale at call time.
        if (prev && api.isTauri && prev.session_id !== bootstrap.session_id) {
          api.closeSession(prev.session_id).catch(() => { /* best-effort */ })
        }
        return bootstrap
      })
      resetProjections()
      setProjectSettings(api.isTauri ? null : browserSettings)
      setWorkspaceTabs([])
      workspaceTabsRef.current = []
      pluginDefaultPendingTabsRef.current.clear()
      setActiveWorkspaceTabId(null)
      setPluginPageTabs([])
      setActivePluginPageKey(null)
      setWorkspaceReadySessionId(null)
      setActiveType('')
      history.clear()
      router.clear()
      if (api.isTauri) {
        api.getProjectSettings(bootstrap.session_id).then(settings => {
          if (generation.currentSession() !== bootstrap.session_id) return
          setProjectSettings(settings)
          installWorkspace(bootstrap, settings)
        }).catch(err => {
          if (generation.currentSession() === bootstrap.session_id) {
            setErrorMsg(`读取编辑器设置失败: ${errorMessage(err)}`)
            const settings = emptyProjectSettings()
            setProjectSettings(settings)
            installWorkspace(bootstrap, settings)
          }
        })
      } else {
        installWorkspace(bootstrap, browserSettings)
      }
    },
    [generation, history, installWorkspace, lookups, resetProjections, router]
  )

  useEffect(() => {
    if (!api.isTauri || startupProjectRequested.current) return
    startupProjectRequested.current = true
    const yamlPath = readLastProjectPath()
    if (!yamlPath) return
    const request = generation.beginProjectRequest()
    api.loadProject(yamlPath).then(bootstrap => {
      if (!generation.isProjectRequestCurrent(request)) return
      adoptSnapshot(bootstrap)
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
    expectedRevision?: number,
  ) => {
    if (
      generation.currentSession() !== sessionId
      || (expectedRevision !== undefined && !generation.isCurrent(sessionId, expectedRevision))
    ) return
    setErrorMsg(`${prefix}: ${errorMessage(err)}`)
  }, [generation])

  useWorkspacePersistence({
    project,
    readySessionId: workspaceReadySessionId,
    tabs: workspaceTabs,
    activeTabId: activeWorkspaceTabId,
    generation,
    setSettings: setProjectSettings,
    setError: setErrorMsg,
  })

  const openProject = useCallback(async () => {
    if (!await flushSourceChanges()) return
    if (!api.isTauri) {
      generation.adopt(MOCK_PROJECT)
      lookups.adopt({ sessionId: MOCK_PROJECT.session_id, revision: MOCK_PROJECT.revision })
      history.clear()
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setProjectSettings(browserSettings)
      return
    }
    const request = generation.beginProjectRequest()
    const yamlPath = await api.pickProjectYaml()
    if (!generation.isProjectRequestCurrent(request) || !yamlPath) return
    setErrorMsg(null)
    try {
      const bootstrap = await api.loadProject(yamlPath)
      if (!generation.isProjectRequestCurrent(request)) return
      rememberLastProject(yamlPath)
      adoptSnapshot(bootstrap)
    } catch (err) {
      if (!generation.isProjectRequestCurrent(request)) return
      setErrorMsg(`打开项目失败: ${errorMessage(err)}`)
    }
  }, [adoptSnapshot, generation, history, lookups])

  const refreshFromBootstrap = useCallback(
    async (bootstrap: ProjectBootstrap) => {
      if (!generation.acceptSnapshot(bootstrap)) return
      lookups.adopt({ sessionId: bootstrap.session_id, revision: bootstrap.revision })
      const dimensions = bootstrap.dimensions
      const validWorkspace = sanitizeProjectWorkspace(
        workspaceToWire(workspaceTabsRef.current, activeWorkspaceTabId), bootstrap.file_types,
        new Set(collectSourceFiles(bootstrap)), bootstrap.dimensions,
      )
      const validTabs = validWorkspace?.tabs ?? []
      workspaceTabsRef.current = validTabs
      setWorkspaceTabs(validTabs)
      const current = router.current
      const sourceFiles = collectSourceFiles(bootstrap)
      const currentTarget = validTabs.find(tab => tab.id === activeWorkspaceTabId)?.dimensionTarget
      const validDimension = currentTarget && bootstrap.dimensions.some(dimension => dimension.name === currentTarget.dimension)
      const keepFile = current && (sourceFiles.includes(current.file) || (!!validDimension && current.file === dimensionViewPath(currentTarget)))
      const nextFile = keepFile ? current.file : sourceFiles[0]
      history.clear()
      setHighlightField(null)
      if (!nextFile) {
        setProject(bootstrap)
        resetProjections()
        return
      }
      if (!current || !keepFile) {
        setProject(bootstrap)
        const option = bootstrap.file_types[nextFile]?.[0]
        const tab = defaultWorkspaceTab(nextFile, option?.name ?? '')
        setWorkspaceTabs(existing => {
          const next = [
            ...existing.filter(item => item.id !== tab.id),
            tab,
          ]
          workspaceTabsRef.current = next
          return next
        })
        navigateWorkspaceTab(tab)
        return
      }
      if (dimensionForFile(dimensions, nextFile) && validDimension) {
        setProject(bootstrap)
        router.replace(current.view === 'table' ? { ...current, dimensionTargetId: currentTarget ? dimensionTargetId(currentTarget) : undefined } : current)
        return
      }
      try {
        const fileRecords = api.isTauri
          ? await fetchFileRecords(
              queryClient, bootstrap.session_id, bootstrap.revision, nextFile,
              () => api.getFileRecords(bootstrap.session_id, nextFile),
            )
          : null
        if (
          !generation.isCurrent(bootstrap.session_id, bootstrap.revision) ||
          (fileRecords && fileRecords.revision !== bootstrap.revision)
        ) return
        setProject(bootstrap)
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
                viewId: DEFAULT_TABLE_VIEW_ID,
                typeFilter: current.coordinate.actual_type,
              })
        } else {
          router.replace(current)
        }
      } catch (err) {
        if (generation.isCurrent(bootstrap.session_id, bootstrap.revision)) {
          setProject(bootstrap)
          reportSessionError(bootstrap.session_id, '刷新项目失败', err)
          router.push({ view: 'table', file: nextFile, viewId: DEFAULT_TABLE_VIEW_ID, typeFilter: bootstrap.file_types[nextFile]?.[0]?.name ?? '' })
        }
      }
    },
    [activeWorkspaceTabId, generation, history, lookups, navigateWorkspaceTab, reportSessionError, resetProjections, router],
  )

  const commitProjectRevision = useCallback((
    sessionId: number,
    revision: number,
    diagnostics: FlatDiagnostic[],
    projectMetadata?: MutationPublicationRequest['projectMetadata'],
  ) => {
    if (!generation.acceptMutation(sessionId, revision)) return false
    lookups.adopt({ sessionId, revision })
    setProject(current => (
      current && current.session_id === sessionId && current.revision <= revision
        ? { ...current, ...projectMetadata, revision, diagnostics }
        : current
    ))
    return true
  }, [generation, lookups])

  const publishMutation = useMutationPublication({
    generation,
    graphDepth: GRAPH_DEPTH,
    graphLimit: GRAPH_LIMIT,
    graphCacheRef,
    setFiles: setFileDataCache,
    setGraphs: setGraphCache,
    acceptRevision: commitProjectRevision,
  })

  const reportWatchError = useCallback((message: string) => {
    setErrorMsg(`监听项目变更失败: ${message}`)
  }, [])
  useProjectWatcher({
    project,
    generation,
    refresh: refreshFromBootstrap,
    reportError: reportSessionError,
    reportWatchError,
  })

  // "新建工程": pick an empty directory, scaffold a minimal Coflow
  // project (mirrors `coflow init`), and open it. The same back-end call
  // refuses to clobber an existing `coflow.yaml` and that diagnostic
  // surfaces here as a clear error banner.
  const newProject = useCallback(async () => {
    if (!await flushSourceChanges()) return
    if (!api.isTauri) {
      setErrorMsg('新建工程仅在桌面环境可用')
      return
    }
    const request = generation.beginProjectRequest()
    const dir = await api.pickProjectDirectory()
    if (!generation.isProjectRequestCurrent(request) || !dir) return
    setErrorMsg(null)
    try {
      const bootstrap = await api.initProject(dir)
      if (!generation.isProjectRequestCurrent(request)) return
      rememberLastProject(projectYamlPath(dir))
      adoptSnapshot(bootstrap)
    } catch (err) {
      if (!generation.isProjectRequestCurrent(request)) return
      setErrorMsg(`新建工程失败: ${errorMessage(err)}`)
    }
  }, [adoptSnapshot, generation])

  // React Query 负责读取去重和竞态；本地缓存继续承载乐观编辑投影。
  useEffect(() => {
    const records = dataQueries.fileQuery.data
    if (records) setFileDataCache(cache => ({ ...cache, [dataQueries.file]: records }))
  }, [dataQueries.file, dataQueries.fileQuery.data])

  useEffect(() => {
    const graph = dataQueries.graphQuery.data ?? dataQueries.mockGraph
    if (graph) setGraphCache(cache => ({ ...cache, [dataQueries.graphKey]: graph }))
  }, [dataQueries.graphKey, dataQueries.graphQuery.data, dataQueries.mockGraph])

  useEffect(() => {
    if (!project) return
    const failure = dataQueries.fileQuery.error
      ? ['读取文件失败', dataQueries.fileQuery.error] as const
      : dataQueries.dimensionQuery.error
        ? ['读取维度文件失败', dataQueries.dimensionQuery.error] as const
        : dataQueries.graphQuery.error
          ? ['读取图谱失败', dataQueries.graphQuery.error] as const
          : null
    if (failure) reportSessionError(project.session_id, failure[0], failure[1])
  }, [dataQueries.dimensionQuery.error, dataQueries.fileQuery.error, dataQueries.graphQuery.error, project, reportSessionError])

  const openFile = useCallback(
    (filePath: string, requestedType = ''): void => {
      if (deferSourceNavigation(() => openFile(filePath, requestedType))) return
      setDocumentSearch('')
      const options = project?.file_types[filePath] ?? []
      const typeName = requestedType || options[0]?.name || ''
      const id = workspaceTabId(filePath, typeName)
      const existing = workspaceTabsRef.current.find(tab => tab.id === id)
      const baseTab = existing ?? defaultWorkspaceTab(filePath, typeName)
      const defaultPluginView = !existing && typeName ? preferredPluginView(typeName) : undefined
      const tab = defaultPluginView
        ? { ...baseTab, viewKind: 'table' as const, viewId: defaultPluginView.key }
        : baseTab
      if (!existing && !defaultPluginView && !pluginsReady) {
        pluginDefaultPendingTabsRef.current.add(tab.id)
      }
      if (!existing) {
        const next = [...workspaceTabsRef.current, tab]
        workspaceTabsRef.current = next
        setWorkspaceTabs(next)
      }
      navigateWorkspaceTab(tab)
    },
    [navigateWorkspaceTab, pluginsReady, project?.file_types]
  )

  useEffect(() => {
    if (!pluginsReady || pluginDefaultPendingTabsRef.current.size === 0) return
    const pending = pluginDefaultPendingTabsRef.current
    let activeChanged = false
    let changed = false
    const next = workspaceTabsRef.current.map(tab => {
      if (!pending.delete(tab.id) || !tab.typeName) return tab
      const pluginView = preferredPluginView(tab.typeName)
      if (!pluginView) return tab
      changed = true
      if (tab.id === activeWorkspaceTabId) activeChanged = true
      return { ...tab, viewKind: 'table' as const, viewId: pluginView.key }
    })
    if (!changed) return
    workspaceTabsRef.current = next
    setWorkspaceTabs(next)
    if (activeChanged && !activePluginPageKey) {
      const active = next.find(tab => tab.id === activeWorkspaceTabId)
      if (active) navigateWorkspaceTab(active)
    }
  }, [activePluginPageKey, activeWorkspaceTabId, navigateWorkspaceTab, pluginRegistry.revision, pluginsReady, workspaceReadySessionId])

  const openDimension = useCallback((target: DimensionTarget) => {
    if (deferSourceNavigation(() => openDimension(target))) return
    const tab = dimensionWorkspaceTab(target)
    const existing = workspaceTabsRef.current.find(candidate => candidate.id === tab.id)
    if (!existing) {
      const next = [...workspaceTabsRef.current, tab]
      workspaceTabsRef.current = next
      setWorkspaceTabs(next)
    }
    navigateWorkspaceTab(existing ?? tab)
  }, [navigateWorkspaceTab])

  const openRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate) => {
      finishActiveDataEdit()
      const id = workspaceTabId(filePath, coordinate.actual_type)
      pluginDefaultPendingTabsRef.current.delete(id)
      const existing = workspaceTabsRef.current.find(tab => tab.id === id)
      const tab: WorkspaceTab = {
        ...(existing ?? defaultWorkspaceTab(filePath, coordinate.actual_type)),
        viewKind: 'record',
        viewId: DEFAULT_RECORD_VIEW_ID,
        coordinate,
      }
      const next = existing
        ? workspaceTabsRef.current.map(item => item.id === id ? tab : item)
        : [...workspaceTabsRef.current, tab]
      workspaceTabsRef.current = next
      setWorkspaceTabs(next)
      navigateWorkspaceTab(tab, coordinate)
    },
    [navigateWorkspaceTab]
  )

  const jumpToDiagnostic = useCallback((target: DiagnosticTarget) => {
    if (target.kind === 'none') return
    if (target.kind === 'table_field' || target.kind === 'record') {
      setHighlightField(target.kind === 'table_field' ? target.field_path : RECORD_HIGHLIGHT_SENTINEL)
      openRecord(target.file_path, target.coordinate)
      return
    }
    const typeName = project?.file_types[target.file_path]?.[0]?.name ?? ''
    const id = workspaceTabId(target.file_path, typeName)
    const existing = workspaceTabsRef.current.find(tab => tab.id === id)
    const tab: WorkspaceTab = {
      ...(existing ?? defaultWorkspaceTab(target.file_path, typeName)),
      viewKind: 'source',
      viewId: DEFAULT_SOURCE_VIEW_ID,
    }
    const next = existing
      ? workspaceTabsRef.current.map(item => item.id === id ? tab : item)
      : [...workspaceTabsRef.current, tab]
    workspaceTabsRef.current = next
    setWorkspaceTabs(next)
    setSourceDiagnosticFocus(previous => ({
      file: target.file_path,
      range: target.range,
      tick: (previous?.tick ?? 0) + 1,
    }))
    navigateWorkspaceTab(tab)
  }, [navigateWorkspaceTab, openRecord, project?.file_types])

  pluginOpenRecordRef.current = (filePath, coordinate, fieldPath) => {
    setHighlightField(fieldPath ?? RECORD_HIGHLIGHT_SENTINEL)
    openRecord(filePath, coordinate)
  }

  // Click on a corner badge (on a record or field): reveal the first
  // matching diagnostic in the bottom panel. Falls back to record-level
  // (fieldPath = null) if there's no exact field-level match.
  const focusDiagnosticForAnchor = useCallback(
    (
      filePath: string,
      recordIdValue: string,
      actualType: string | null,
      fieldPath: string | null,
    ) => {
      if (!project) return
      const source = project.diagnostics
      // Prefer field-level; only fall back to record-level when the caller
      // asked for a field. When they asked for the whole record we take the
      // first diagnostic on it regardless of field.
      let hit = fieldPath
        ? source.find(d => diagnosticMatchesAnchor(d, filePath, recordIdValue, actualType, fieldPath))
        : undefined
      if (!hit) {
        hit = source.find(d => diagnosticMatchesAnchor(d, filePath, recordIdValue, actualType, null))
      }
      if (!hit) return
      setDiagFocus(prev => ({
        key: diagnosticKey(hit!),
        tick: (prev?.tick ?? 0) + 1,
      }))
    },
    [project],
  )

  const navigateReference = useCallback((targetType: string, targetKey: string) => {
    void lookups.loadRefTargets(targetType).then(result => {
      if (!result.ok) {
        if (result.reason === 'failed') {
          setErrorMsg(`读取引用目标失败: ${result.error ?? targetType}`)
        }
        return
      }
      const target = result.value.find(item => (
        item.coordinate.id === targetKey
        && item.coordinate.actual_type === targetType
      )) ?? result.value.find(item => item.coordinate.id === targetKey)
      if (!target) {
        setErrorMsg(`引用记录 ${targetKey} 未找到`)
        return
      }
      const { coordinate, file_path: filePath } = target
      const id = workspaceTabId(filePath, coordinate.actual_type)
      const existing = workspaceTabsRef.current.find(tab => tab.id === id)
      const base = existing ?? defaultWorkspaceTab(
        filePath,
        coordinate.actual_type,
      )
      const tab = base.viewKind === 'record' ? { ...base, coordinate } : base
      const next = existing
        ? workspaceTabsRef.current.map(item => item.id === id ? tab : item)
        : [...workspaceTabsRef.current, tab]
      workspaceTabsRef.current = next
      setWorkspaceTabs(next)
      navigateWorkspaceTab(tab, coordinate)
      if (tab.viewKind !== 'record') openInspector(filePath, coordinate)
    })
  }, [lookups, navigateWorkspaceTab, openInspector, project?.file_types])

  // 引用查看沿用同一详情组件和写入控制器，避免改变底层文件与选区。
  const openReference = useCallback((targetType: string, targetKey: string) => {
    if (deferDataNavigation(() => openReference(targetType, targetKey))) return
    const identity = generation.currentIdentity()
    void lookups.loadRefTargets(targetType).then(result => {
      if (!identity || !generation.isCurrent(identity.sessionId, identity.revision)) return
      if (!result.ok) { setErrorMsg('读取引用目标失败'); return }
      const target = result.value.find(item => item.coordinate.id === targetKey && item.coordinate.actual_type === targetType)
        ?? result.value.find(item => item.coordinate.id === targetKey)
      if (!target) { setErrorMsg(`引用记录 ${targetKey} 未找到`); return }
      setReferenceHistory(current => [...current, target])
    })
  }, [lookups, generation])

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
      setFileDataCache(projectedCache)
      const projectedGraphs = projectGraphRows(
        graphCacheRef.current,
        current.revision,
        [projection.row],
      )
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
        setFileDataCache(nextCache)
        const nextGraphs = projectGraphRows(
          graphCacheRef.current,
          latest.revision,
          [rollback.row],
        )
        setGraphCache(nextGraphs)
        appliedIdentity = null
      },
    }
  }, [generation])

  const mutationPort = useMemo<EditorMutationPort>(() => ({
    applyGraphPositions,
    currentGeneration: () => api.isTauri ? generation.currentIdentity() : null,
    publish: publishMutation,
    fileRecordsForRow,
    rebindCoordinate,
    removeCoordinate,
    recoverPublication: (request, error) => {
      if (!commitProjectRevision(request.sessionId, request.revision, request.diagnostics, request.projectMetadata)) {
        return false
      }
      window.setTimeout(() => {
        publishMutation(request).catch(retryError => {
          reportSessionError(
            request.sessionId,
            '后台刷新仍然失败',
            retryError ?? error,
            request.revision,
          )
        })
      }, 250)
      return true
    },
    reportError: (sessionId, prefix, error, expectedRevision) => {
      reportSessionError(sessionId, prefix, error, expectedRevision)
    },
    optimisticWriteField,
  }), [applyGraphPositions, commitProjectRevision, fileRecordsForRow, generation, optimisticWriteField, publishMutation, rebindCoordinate, removeCoordinate, reportSessionError])
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
      record_id: row.coordinate.id,
      field: row.field,
      dimension: data.dimension,
      variant,
      path: [],
    }
    const updateCache = (value: DimensionValueState, revision: number) => {
      const identity = generation.currentIdentity()
      if (identity?.revision !== revision) return
      queryClient.setQueryData<api.DimensionFileRecords>(
        editorQueryKeys.dimensionRecords(identity.sessionId, revision, data.file_path),
        current => {
          const base = current ?? data
          return {
          ...base,
          revision,
          variants: [...new Set([...base.variants, variant])],
          rows: base.rows.map(currentRow => sameCoordinate(currentRow.coordinate, row.coordinate)
            && currentRow.field === row.field
            ? { ...currentRow, values: { ...currentRow.values, [variant]: value },
                sources: { ...currentRow.sources, [variant]: value.kind === 'missing' ? null
                  : currentRow.sources[variant] ?? data.file_path.slice(`@dimension/${data.dimension}/`.length) } }
            : currentRow),
          }
        },
      )
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
    async (filePath: string, coordinate: RecordCoordinate, newId: string) => {
      const groups = projectSettings?.record_groups[filePath]?.[coordinate.actual_type] ?? []
      const result = await mutations.renameRecord(filePath, coordinate, newId)
      if (result) {
        saveRecordGroups(
          filePath,
          coordinate.actual_type,
          replaceGroupedCoordinate(groups, coordinate, result.coordinate),
        )
        const byView = projectSettings?.table_row_heights[filePath]?.[coordinate.actual_type] ?? {}
        const previousKey = coordinateId(coordinate), nextKey = coordinateId(result.coordinate)
        const migrated = Object.fromEntries(Object.entries(byView).map(([view, heights]) => {
          const next = { ...heights }
          if (next[previousKey] !== undefined) { next[nextKey] = next[previousKey]; delete next[previousKey] }
          return [view, next]
        }))
        setProjectSettings(current => current ? { ...current, table_row_heights: {
          ...current.table_row_heights, [filePath]: { ...current.table_row_heights[filePath], [coordinate.actual_type]: migrated },
        } } : current)
        const identity = generation.currentIdentity()
        if (api.isTauri && identity) {
          rowHeightSaveQueue.current = rowHeightSaveQueue.current.then(async () => {
            if (generation.currentSession() !== identity.sessionId) return
            for (const [view, heights] of Object.entries(migrated)) {
              await api.setTableRowHeights(identity.sessionId, filePath, coordinate.actual_type, view, heights as Record<string, number>)
            }
          }).catch(error => { if (generation.currentSession() === identity.sessionId) setErrorMsg(`保存行高失败：${errorMessage(error)}`) })
        }
      }
      return result
    },
    [mutations, projectSettings, saveRecordGroups, generation, setProjectSettings],
  )

  const insertRecord = useCallback(
    async (filePath: string, recordId: string, actualType: string, fields: FieldValue) => {
      await mutations.insertRecord(filePath, recordId, actualType, fields)
    },
    [mutations],
  )
  const deleteRecords = useCallback(
    async (filePath: string, coordinates: readonly RecordCoordinate[]) => {
      for (const coordinate of coordinates) {
        await mutations.deleteRecord(filePath, coordinate)
      }

      const actualTypes = new Set(coordinates.map(coordinate => coordinate.actual_type))
      for (const actualType of actualTypes) {
        const groups = projectSettings?.record_groups[filePath]?.[actualType] ?? []
        saveRecordGroups(
          filePath,
          actualType,
          removeRecordsFromGroups(groups, coordinates),
        )
      }
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

  useEffect(() => {
    setPluginDataBridge({
      currentIdentity: () => generation.currentIdentity(),
      getSchema: api.getPluginSchema,
      getRecordsByType: api.getPluginRecordsByType,
      getFileRecords: async (sessionId, filePath) => {
        if (api.isTauri) {
          const identity = generation.currentIdentity()
          if (!identity || identity.sessionId !== sessionId) throw new Error('当前项目会话已变更')
          return fetchFileRecords(queryClient, sessionId, identity.revision, filePath, () => (
            api.getFileRecords(sessionId, filePath)
          ))
        }
        const records = fileDataCacheRef.current[filePath]
        if (!records) throw new Error(`找不到文件 ${filePath}`)
        return records
      },
      searchRecords: async (sessionId, query, mode, limit) => {
        if (!api.isTauri) {
          const identity = generation.currentIdentity()
          if (!identity || identity.sessionId !== sessionId) throw new Error('当前项目会话已变更')
          return searchMockRecords(
            fileDataCacheRef.current,
            sessionId,
            identity.revision,
            query,
            mode,
            limit,
          )
        }
        const result = await api.searchRecords(sessionId, query, mode, limit)
        return {
          sessionId,
          revision: result.revision,
          data: {
            hits: result.hits.map(hit => ({
              filePath: hit.file_path,
              coordinate: hit.coordinate,
              fieldPath: hit.field_path,
              preview: hit.preview,
            })),
            truncated: result.truncated,
          },
        }
      },
      mutate: async (request: PluginMutationRequest) => {
        switch (request.kind) {
          case 'write_field':
            await mutations.writeFieldBatch(request.filePath, [{
              coordinate: request.coordinate,
              field_path: request.fieldPath,
              new_value: request.value,
            }])
            break
          case 'write_fields':
            await mutations.writeFields(request.filePath, request.coordinates, request.fieldPath, request.value)
            break
          case 'write_field_batch':
            await mutations.writeFieldBatch(request.filePath, request.writes)
            break
          case 'edit_collection':
            await editCollection(request.filePath, request.coordinate, request.fieldPath, request.edit)
            break
          case 'rename_record':
            await renameRecord(request.filePath, request.coordinate, request.newId)
            break
          case 'insert_record':
            await insertRecord(request.filePath, request.recordId, request.actualType, request.fields)
            break
          case 'delete_record':
            await deleteRecords(request.filePath, [request.coordinate])
            break
          case 'swap_records':
            await swapRecords(request.filePath, request.first, request.second)
            break
          case 'move_record':
            await moveRecord(request.filePath, request.coordinate, request.targetIndex)
            break
          case 'transfer_record':
            await transferRecord(request.sourceFile, request.destinationFile, request.coordinate, request.targetIndex)
            break
        }
      },
    })
    return () => setPluginDataBridge(null)
  }, [deleteRecords, editCollection, generation, insertRecord, moveRecord, mutations, renameRecord, swapRecords, transferRecord])

  const undo = useCallback(async () => {
    await mutations.undo()
  }, [mutations])

  const redo = useCallback(async () => {
    await mutations.redo()
  }, [mutations])

  // 图视图“新建节点+建立引用”走同一条历史记录，一次撤销整体回退。
  const runHistoryBatch = useCallback(
    (operation: () => Promise<void>) => mutations.withHistoryBatch(operation),
    [mutations],
  )

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
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'g') {
        e.preventDefault()
        finishActiveDataEdit()
        setGitDiffOpen(true)
        afterSourceSave(() => setGitDiffActive(true))
        setActivePluginPageKey(null)
        setActivePane('changes')
      }
      // `?` only toggles help when not focused inside a text-editing control,
      // otherwise typing `?` into inputs/search boxes would steal focus.
      if (e.key === '?' && !isNativeEditorTarget(e.target, false)) setShowHelp(v => !v)
      if (e.key === 'Escape') setShowHelp(false)
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [router, undo, redo])

  const currentRoute = router.current
  const activePluginPage = activePluginPageKey
    ? pluginRegistry.pages.find(page => page.key === activePluginPageKey)
    : undefined
  const pluginPageContext = useMemo<PluginPageContext | null>(() => (
    activePluginPage
      ? {
          identity: project ? { sessionId: project.session_id, revision: project.revision } : null,
          pageId: activePluginPage.id,
          openPage: pageId => openPluginPageTab(activePluginPage.pluginId, pageId),
          closePage: pageId => closePluginPageTab(`${activePluginPage.pluginId}/${pageId ?? activePluginPage.id}`),
        }
      : null
  ), [activePluginPage, closePluginPageTab, openPluginPageTab, project])
  const activeFile = currentRoute?.file ?? null
  const activeSchemaFile = activeFile?.endsWith('.cft') ?? false
  useEffect(() => {
    if (!currentRoute) return
    const tab = workspaceTabsRef.current.find(
      candidate => candidate.id === (currentRoute.view === 'table' ? currentRoute.dimensionTargetId ?? activeWorkspaceTabId : activeWorkspaceTabId)
        && candidate.filePath === currentRoute.file,
    )
    if (tab?.dimensionTarget) setDimensionView(tab.viewKind === 'record' ? 'record' : 'table')
  }, [currentRoute, activeWorkspaceTabId])
  useEffect(() => {
    if (!currentRoute) return
    if (currentRoute.view === 'table' && currentRoute.dimensionTargetId) {
      const tab = workspaceTabsRef.current.find(item => item.id === currentRoute.dimensionTargetId)
      if (tab?.dimensionTarget && tab.filePath === currentRoute.file) setActiveWorkspaceTabId(tab.id)
      return
    }
    if (!pluginsReady && currentRoute.viewId.includes('/')) return
    const typeName = currentRoute.view === 'record'
      ? currentRoute.coordinate.actual_type
      : currentRoute.typeFilter ?? ''
    const id = workspaceTabId(currentRoute.file, typeName)
    if (!workspaceTabs.some(tab => tab.id === id)) return
    setActiveWorkspaceTabId(id)
    if (typeName) setActiveType(typeName)
    if (!typeName) return
    setWorkspaceTabs(current => {
      let changed = false
      const next = current.map(tab => {
        if (tab.id !== id) return tab
        const pluginView = pluginRegistry.views.some(view => view.key === currentRoute.viewId)
        const viewKind = pluginView ? 'table' : currentRoute.view
        const viewId = currentRoute.viewId
        const coordinate = viewKind === 'record' && currentRoute.view === 'record'
          ? currentRoute.coordinate
          : tab.coordinate
        const sameRecordCoordinate = (!tab.coordinate && !coordinate)
          || (!!tab.coordinate && !!coordinate && sameCoordinate(tab.coordinate, coordinate))
        if (
          tab.viewKind === viewKind
          && tab.viewId === viewId
          && (viewKind !== 'record' || sameRecordCoordinate)
        ) return tab
        changed = true
        return { ...tab, viewKind, viewId, coordinate }
      })
      if (!changed) return current
      workspaceTabsRef.current = next
      return next
    })
  }, [currentRoute, pluginRegistry.views, pluginsReady, workspaceTabs])
  // 优先保留乐观编辑投影；首次读取直接显示查询结果，不等待 effect 再渲染一轮。
  const activeFileData = activeFile
    ? fileDataCache[activeFile] ?? dataQueries.fileQuery.data ?? null
    : null
  const activeDimensionTarget = workspaceTabs.find(tab => tab.id === (currentRoute?.view === 'table' ? currentRoute.dimensionTargetId : undefined))?.dimensionTarget
  const activeDimensionData = activeFile
    ? dataQueries.dimensionQuery.data ?? dataQueries.mockDimension ?? null
    : null
  const selectedDimensionData = activeDimensionData && activeDimensionTarget
    ? activeDimensionData : null
  useEffect(() => {
    if (!activeDimensionData || !activeWorkspaceTabId || currentRoute?.view !== 'table'
      || activeWorkspaceTabId !== currentRoute.dimensionTargetId) return
    setWorkspaceTabs(current => {
      let changed = false
      const next = current.map(tab => {
        if (tab.id !== activeWorkspaceTabId || !tab.dimensionTarget) return tab
        const viewId = dimensionView === 'record' ? DEFAULT_RECORD_VIEW_ID : DEFAULT_TABLE_VIEW_ID
        if (tab.viewKind === dimensionView && tab.viewId === viewId) return tab
        changed = true
        return { ...tab, viewKind: dimensionView, viewId }
      })
      if (!changed) return current
      workspaceTabsRef.current = next
      return next
    })
  }, [activeDimensionData, activeWorkspaceTabId, currentRoute, dimensionView])
  const recordGroups = projectSettings?.record_groups[activeFile ?? '']?.[activeType] ?? EMPTY_RECORD_GROUPS
  const activeTypeOption = useMemo(
    () => project?.file_types[activeFile ?? '']?.find(option => option.name === activeType) ?? null,
    [project?.file_types, activeFile, activeType],
  )
  const isSingletonType = activeTypeOption?.is_singleton ?? false
  const pluginViews = usePluginViews(activeType)
  const activePluginView = currentRoute
    ? pluginViews.find(view => view.key === currentRoute.viewId)
    : undefined
  const activeViewKind = activePluginView
    ? 'plugin'
    : currentRoute?.view
  useContentMotion(viewContainerRef, JSON.stringify([activeWorkspaceTabId, currentRoute?.viewId, activeViewKind]))
  const activeRecordCoordinate = useMemo(() => (
    currentRoute?.view === 'record'
      ? currentRoute.coordinate
      : activeFileData?.records.find(record => (
        !activeType || recordActualType(record) === activeType
      ))?.coordinate ?? null
  ), [currentRoute, activeFileData, activeType])
  const pluginSelection = useMemo(() => {
    if (!inspectorSelection) return null
    if (inspectorSelection.kind === 'record') {
      return {
        kind: 'record' as const,
        filePath: inspectorSelection.filePath,
        coordinates: inspectorSelection.coordinates.map(coordinate => ({ ...coordinate })),
      }
    }
    return {
      kind: 'field' as const,
      filePath: inspectorSelection.filePath,
      coordinate: { ...inspectorSelection.coordinate },
      fieldPath: inspectorSelection.fieldPath.map(segment => ({ ...segment })),
    }
  }, [inspectorSelection])
  const pluginActiveContext = useMemo<PluginActiveContext>(() => ({
    identity: project ? { sessionId: project.session_id, revision: project.revision } : null,
    filePath: activeFile,
    typeName: activeType || null,
    selection: pluginSelection,
    surface: activePluginPage ? 'page' : activePluginSidebar ? 'sidebar' : 'editor',
  }), [activeFile, activePluginPage, activePluginSidebar, activeType, pluginSelection, project])

  useEffect(() => {
    publishPluginEvent('project', {
      identity: project ? { sessionId: project.session_id, revision: project.revision } : null,
    })
  }, [project?.session_id])
  useEffect(() => {
    if (project) publishPluginEvent('data', { sessionId: project.session_id, revision: project.revision })
  }, [project?.session_id, project?.revision])
  useEffect(() => {
    publishPluginEvent('selection', { selection: pluginSelection })
  }, [pluginSelection])
  useEffect(() => {
    publishPluginEvent('surface', activePluginPage
      ? { kind: 'document', id: activePluginPage.key }
      : activePluginSidebar
        ? { kind: 'sidebar', id: activePluginSidebar.key }
        : {
            kind: 'view',
            id: activePluginView?.key ?? currentRoute?.viewId ?? null,
            ...(activeFile ? { filePath: activeFile } : {}),
            ...(activeType ? { typeName: activeType } : {}),
          })
  }, [activeFile, activePluginPage, activePluginSidebar, activePluginView, activeType, currentRoute?.viewId])

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      // 输入控件保留普通编辑按键，但允许带主修饰键的插件快捷键。
      if (isNativeEditorTarget(event.target, false) && !event.ctrlKey && !event.metaKey && !event.altKey) return
      dispatchPluginKeybinding(event, pluginActiveContext)
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [pluginActiveContext])

  useEffect(() => {
    setInspectorSelection(current => {
      if (!current) return current
      if (router.current?.view === 'graph') return current
      if (!activeFileData || current.filePath !== activeFileData.file_path) return null
      const inActiveType = (coordinate: RecordCoordinate) => activeFileData.records.some(record => (
        sameCoordinate(record.coordinate, coordinate)
        && (!activeType || recordActualType(record) === activeType)
      ))
      if (current.kind === 'value') {
        if (inActiveType(current.coordinate) && inActiveType(current.rangeAnchor.coordinate)) return current
        if (inActiveType(current.coordinate)) {
          return valueSelection(current.filePath, current.coordinate, current.fieldPath)
        }
        if (inActiveType(current.rangeAnchor.coordinate)) {
          return valueSelection(current.filePath, current.rangeAnchor.coordinate, current.rangeAnchor.fieldPath)
        }
        return null
      }
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
    finishActiveDataEdit()
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
  const recoveryRequired = recoverySession !== null && recoverySession === project?.session_id
  const readOnly = recoveryRequired || (activeSchemaFile ? false : !isEditableFile(activeFileData))
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
      next[filePath] = (next[filePath] ?? [])
        .map(option => ({
          ...option,
          record_count: counts.get(option.name) ?? 0,
        }))
        .filter(option => option.record_count > 0)
    }
    for (const [filePath, options] of Object.entries(next)) {
      if (filePath in fileDataCache) continue
      next[filePath] = options?.filter(option => option.record_count > 0)
    }
    return next
  }, [fileDataCache, project])
  const fileDiagnostics = useMemo(
    () => activeFile && project ? project.diagnostics.filter(d => diagnosticFilePath(d) === activeFile) : [],
    [activeFile, project?.diagnostics],
  )
  // 图能力按文件快照引用缓存；会话切换不会复用同路径、同版本的旧项目结果。
  const graphSupported = useMemo(
    () => activeFileData ? graphSupportForFile(activeFileData) : false,
    [activeFileData],
  )
  // View tabs (default + custom) for the active (file, type).
  const viewTabs = useMemo(
    () => activeFile
      ? viewTabsFor(projectSettings, activeFile, activeType, isSingletonType, graphSupported)
      : [],
    [projectSettings, activeFile, activeType, isSingletonType, graphSupported],
  )
  // The view the current route resolves to (default reserved id or custom uuid).
  const resolvedView = useMemo(
    () => activeFile && activeType && currentRoute && !activePluginView
      ? resolveView(projectSettings, activeFile, activeType, currentRoute.viewId, isSingletonType)
      : null,
    [projectSettings, activeFile, activeType, currentRoute, isSingletonType, activePluginView],
  )
  // Fields the custom view restricts to (undefined = show all).
  const visibleFields = useMemo(
    () => (resolvedView ? visibleFieldsFor(resolvedView) : undefined),
    [resolvedView],
  )
  // Custom-view group filter applied to the table's records (whole-list
  // filter, not a collapse). Returns the file data unchanged when the view
  // has no valid group filter.
  const viewFilteredFileData = useMemo(() => {
    if (!activeFileData || !resolvedView || !resolvedView.groupFilter) return activeFileData
    const predicate = groupFilterPredicate(resolvedView, recordGroups)
    return { ...activeFileData, records: activeFileData.records.filter(row => predicate(row.coordinate)) }
  }, [activeFileData, resolvedView, recordGroups])
  const pluginViewContext = useMemo<PluginViewContext | null>(() => {
    if (!project || !activeFileData || !activeFile || !activeType || !activePluginView) return null
    const filtered = activeFileData.records.filter(row => recordActualType(row) === activeType)
    const includeFieldValues = activePluginView.includeFieldValues !== false
    // 一次克隆整批字段，比逐记录 structuredClone 少一次次调用开销，同时保持插件隔离。
    const clonedFields = includeFieldValues ? structuredClone(filtered.map(row => row.fields)) : []
    const records: PluginRecordData[] = filtered.map((row, index) => ({
      filePath: activeFile,
      coordinate: { ...row.coordinate },
      ...(includeFieldValues ? { fields: clonedFields[index] } : {}),
    }))
    return {
      identity: { sessionId: project.session_id, revision: project.revision },
      filePath: activeFile,
      typeName: activeType,
      records,
      readOnly: readOnly || activePluginView.readOnly === true,
      openPage: pageId => openPluginPageTab(activePluginView.pluginId, pageId),
    }
  }, [activeFile, activeFileData, activePluginView, activeType, openPluginPageTab, project, readOnly])
  // Graph root filter: keep group-member roots plus everything reachable from
  // them via edges (only the roots are constrained, per design §4.2).
  const viewFilteredGraph = useMemo(() => {
    if (!activeGraph || !resolvedView || !resolvedView.groupFilter) return activeGraph
    const predicate = groupFilterPredicate(resolvedView, recordGroups)
    return reachableGraph(activeGraph, predicate)
  }, [activeGraph, resolvedView, recordGroups])
  // Choices offered by the view editor dialog.
  const viewEditorFields = useMemo(
    () => activeFileData?.columns.filter(column => column.type_names.includes(activeType)).map(column => column.name) ?? [],
    [activeFileData, activeType],
  )
  // Relations are derived from record annotations (not the graph, which may
  // not be loaded when the dialog opens from the table view).
  const viewEditorRelations = useMemo(
    () => activeFileData
      ? relationFieldNames(
          activeFileData.records.filter(row => recordActualType(row) === activeType),
        )
      : [],
    [activeFileData, activeType],
  )
  // Create or update a custom view: merge into the (file,type) list and save.
  const submitViewConfig = useCallback(async (view: ViewConfig) => {
    if (!activeFile || !activeType) return
    const existing = projectSettings?.views[activeFile]?.[activeType] ?? []
    const next = existing.some(v => v.id === view.id)
      ? existing.map(v => (v.id === view.id ? view : v))
      : [...existing, view]
    await saveViews(activeFile, activeType, next)
    if (currentRoute?.viewId !== view.id) switchView({ ...view, isDefault: view.id === DEFAULT_TABLE_VIEW_ID })
  }, [activeFile, activeType, projectSettings, saveViews, currentRoute?.viewId])

  // Delete a custom view; if the current route pointed at it, fall back to the
  // default table view (its column widths vanish with the ViewConfig).
  const deleteView = useCallback(async (viewId: string) => {
    if (!activeFile || !activeType) return
    const existing = projectSettings?.views[activeFile]?.[activeType] ?? []
    try { await saveViews(activeFile, activeType, existing.filter(v => v.id !== viewId)) }
    catch (error) { setErrorMsg(`删除视图失败：${errorMessage(error)}`); return }
    if (currentRoute?.viewId === viewId) {
      router.replace({ view: 'table', file: activeFile, viewId: DEFAULT_TABLE_VIEW_ID, typeFilter: activeType })
    }
  }, [activeFile, activeType, projectSettings, saveViews, currentRoute, router])
  // Record counts shown next to the search bar across all views. `typeCount`
  // is the number of records of the active type in the current file;
  // `matchedCount` additionally applies the global search filter (matches
  // record key, field names, or field value summaries — the union of what
  // Table and Record views each filter on so the count stays honest for
  // both).
  const { typeCount, matchedCount } = useMemo(() => {
    if (!viewFilteredFileData) return { typeCount: 0, matchedCount: 0 }
    const inType = activeType
      ? viewFilteredFileData.records.filter(r => recordActualType(r) === activeType)
      : viewFilteredFileData.records
    if (!documentSearch.trim()) return { typeCount: inType.length, matchedCount: inType.length }
    const matchesSearch = keyOnlySearch ? recordMatchesKeySearch : recordMatchesFullTextSearch
    const matched = inType.filter(record => matchesSearch(record, documentSearch))
    return { typeCount: inType.length, matchedCount: matched.length }
  }, [viewFilteredFileData, activeType, currentRoute?.view, documentSearch, keyOnlySearch])

  // A hidden row must not remain keyboard-selected after search narrows the
  // active set. Close a hidden table inspector, and keep record mode on the
  // first record that is still visible.
  useEffect(() => {
    if (!viewFilteredFileData || !currentRoute || !documentSearch.trim()) return
    const matchesSearch = keyOnlySearch ? recordMatchesKeySearch : recordMatchesFullTextSearch
    const visible = viewFilteredFileData.records.filter(record => (
      (!activeType || recordActualType(record) === activeType)
      && matchesSearch(record, documentSearch)
    ))
    setInspectorSelection(current => {
      if (!current || current.filePath !== currentRoute.file) return current
      if (current.kind === 'value') {
        const contains = (coordinate: RecordCoordinate) => visible.some(record => sameCoordinate(record.coordinate, coordinate))
        if (contains(current.coordinate) && contains(current.rangeAnchor.coordinate)) return current
        if (contains(current.coordinate)) return valueSelection(current.filePath, current.coordinate, current.fieldPath)
        if (contains(current.rangeAnchor.coordinate)) {
          return valueSelection(current.filePath, current.rangeAnchor.coordinate, current.rangeAnchor.fieldPath)
        }
        return null
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
      router.replace({ view: 'record', file: currentRoute.file, viewId: currentRoute.viewId, coordinate: visible[0].coordinate })
    }
  }, [viewFilteredFileData, activeType, currentRoute, documentSearch, router, keyOnlySearch])

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
    (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], mode: ValueSelectionMode = 'replace') => {
      if (currentRoute?.view !== 'table') return
      if (deferDataNavigation(() => tableOnSelectValue(coordinate, fieldPath, mode))) return
      // 表格在 pointerdown 阶段提交旧目标；选择同步不能再次 blur 刚获得焦点的输入。
      setInspectorSelection(current => updateValueSelection(
        current,
        currentRoute.file,
        coordinate,
        fieldPath,
        mode,
      ))
    },
    [currentRoute?.view, currentRoute?.file],
  )
  const [inspectorValueCells, setInspectorValueCells] = useState<SelectedCells>([])
  const tableOnValueSelectionCellsChange = useCallback((cells: SelectedCells) => {
    setInspectorValueCells(current => sameValueCells(current, cells) ? current : cells)
  }, [])
  const tableOnWriteFieldBatch = useCallback(
    async (writes: readonly BatchWriteFieldInput[]) => {
      if (currentRoute?.view === 'table') {
        await mutations.writeFieldBatch(currentRoute.file, writes)
      }
    },
    [currentRoute?.view, currentRoute?.file, mutations],
  )

  const closeWorkspaceTab = useCallback((id: string): void => {
    if (deferSourceNavigation(() => closeWorkspaceTab(id))) return
    const index = workspaceTabs.findIndex(tab => tab.id === id)
    if (index < 0) return
    pluginDefaultPendingTabsRef.current.delete(id)
    const remaining = workspaceTabs.filter(tab => tab.id !== id)
    workspaceTabsRef.current = remaining
    setWorkspaceTabs(remaining)
    if (id !== activeWorkspaceTabId) return
    const next = remaining[Math.min(index, remaining.length - 1)]
    if (activePluginPageKey) {
      // 插件页保持在前台，同时把被遮挡的工作区路由切到下一个有效标签。
      if (!next) {
        setActiveWorkspaceTabId(null)
        setActiveType('')
        closeInspector()
        router.clear()
        return
      }
      setActiveWorkspaceTabId(next.id)
      setActiveType(next.typeName)
      if (next.dimensionTarget) setDimensionView(next.viewKind === 'record' ? 'record' : 'table')
      const fallbackCoordinate = fileDataCacheRef.current[next.filePath]?.records.find(
        row => recordActualType(row) === next.typeName,
      )?.coordinate
      router.replace(routeForWorkspaceTab(next, fallbackCoordinate))
      return
    }
    if (!next) {
      setActiveWorkspaceTabId(null)
      setActiveType('')
      closeInspector()
      router.clear()
      return
    }
    navigateWorkspaceTab(next)
  }, [activePluginPageKey, activeWorkspaceTabId, closeInspector, navigateWorkspaceTab, router, workspaceTabs])

  const focusFileTree = useCallback(() => {
    const tree = sidebarRef.current?.querySelector<HTMLElement>('.file-tree')
    const target = tree?.querySelector<HTMLElement>('[role="treeitem"][aria-selected="true"]')
      ?? tree?.querySelector<HTMLElement>('[role="treeitem"]')
    target?.focus({ preventScroll: true })
  }, [])
  const focusDocumentSearch = useCallback(() => {
    documentSearchRef.current?.focus({ preventScroll: true })
    documentSearchRef.current?.select()
  }, [])
  const focusDocumentTabs = useCallback(() => {
    document.querySelector<HTMLElement>('.view-picker summary, .document-view-tabs .tab-btn.active')
      ?.focus({ preventScroll: true })
  }, [])
  const focusActiveView = useCallback(() => {
    const target = viewContainerRef.current?.querySelector<HTMLElement>(
      '.table-scroll, .rv-sidebar-item.selected, .rv-main, .graph-view-wrap',
    )
    target?.focus({ preventScroll: true })
  }, [])
  const currentInspectorOrigin = useCallback(() => {
    if (inspectorSelection?.kind !== 'value' || selectedCellCount(inspectorValueCells) !== 1) return null
    const cells = viewContainerRef.current?.querySelectorAll<HTMLElement>('.table-scroll td.selected-cell')
    return cells?.length === 1 ? cellMotionOrigin(cells[0]) : null
  }, [inspectorSelection, inspectorValueCells])
  const focusInspector = useCallback(() => {
    if (!inspectorDialogOpen) setInspectorOrigin(currentInspectorOrigin())
    setInspectorDialogOpen(true)
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>('.inspector-panel:not(.collapsed) .inspector-body')
        ?.focus({ preventScroll: true })
    })
  }, [currentInspectorOrigin, inspectorDialogOpen])
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
      buildStatusRequestRef.current += 1
      setBuildPending(false)
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
    const request = ++buildStatusRequestRef.current
    if (!project || !api.isTauri) {
      setBuildPending(false)
      return
    }
    setBuildPending(false)
    const timer = window.setTimeout(() => {
      api.buildProjectStatus(project.session_id).then(changed => {
        if (buildStatusRequestRef.current === request) setBuildPending(changed)
      }).catch(() => {
        if (buildStatusRequestRef.current === request) setBuildPending(false)
      })
    }, 200)
    return () => window.clearTimeout(timer)
  }, [project?.session_id, project?.revision])

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
  function structureError(sessionId: number, prefix: string, error: unknown) {
    if (generation.currentSession() !== sessionId) return
    if (isEditorError(error) && error.kind === 'committed') {
      setFileActionDialog(null)
      setRecoverySession(sessionId)
      setErrorMsg(`操作已完成，项目刷新失败。${errorMessage(error)}`)
    } else setErrorMsg(`${prefix}: ${errorMessage(error)}`)
  }
  async function recoverProject() {
    const id = generation.currentSession()
    if (id === null) return
    try {
      const snapshot = await api.reloadSession(id)
      await refreshFromBootstrap(snapshot)
      setRecoverySession(null)
      setErrorMsg(null)
    } catch (error) { setErrorMsg(`重新加载失败: ${errorMessage(error)}`) }
  }

  const addProjectInput = useCallback(async (kind: 'schema' | 'data', directory: boolean) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    if (!await flushSourceChanges()) return
    const path = await api.pickProjectInput(kind, directory)
    if (!path || !generation.isCurrent(identity.sessionId, identity.revision)) return
    try {
      const bootstrap = await api.addProjectInput(identity.sessionId, kind, path)
      await refreshFromBootstrap(bootstrap)
    } catch (error) {
      structureError(identity.sessionId, '添加来源失败', error)
    }
  }, [generation, refreshFromBootstrap])
  const createRecordFromTree = useCallback(async (filePath: string, actualType: string) => {
    const identity = generation.currentIdentity()
    if (!identity) return
    try {
      const data = fileDataCache[filePath]?.revision === identity.revision
        ? fileDataCache[filePath]
        : await fetchFileRecords(queryClient, identity.sessionId, identity.revision, filePath, () => (
            api.getFileRecords(identity.sessionId, filePath)
          ))
      if (!generation.isCurrent(identity.sessionId, identity.revision)) return
      openFile(filePath, actualType)
      setTreeRecordDraft({ filePath, actualType: actualType || data.type_names[0] || '', data })
    } catch (error) {
      setErrorMsg(`读取文件失败: ${errorMessage(error)}`)
    }
  }, [fileDataCache, generation, openFile])
  const createFileFromTree = useCallback((parentPath: string, sourceKind: 'schema' | 'data') => {
    setFileActionDialog({ kind: 'create', parentPath, sourceKind })
  }, [])
  const deleteEntryFromTree = useCallback((path: string) => {
    setFileActionDialog({ kind: 'delete', path })
  }, [])
  const tableOnEnterInspector = useCallback(() => {
    setInspectorOrigin(currentInspectorOrigin())
    setInspectorDialogOpen(true)
    setInspectorFocusRequest(request => request + 1)
  }, [currentInspectorOrigin])
  const inspectorOnExitKeyboardNavigation = useCallback(() => {
    if (currentRoute?.view === 'table') setTableFocusRequest(request => request + 1)
    else focusActiveView()
  }, [currentRoute?.view, focusActiveView])
  useEffect(() => { setInspectorDialogOpen(false); setInspectorOrigin(null) }, [activeFile])
  const tableColumnWidths = useMemo(() => (
    resolvedView?.kind === 'table'
      ? definedColumnWidths(resolvedView.columnWidths)
      : undefined
  ), [resolvedView])
  const tableRowHeights = useMemo(() => currentRoute && activeType && resolvedView
    ? projectSettings?.table_row_heights[currentRoute.file]?.[activeType]?.[resolvedView.id]
    : undefined, [projectSettings?.table_row_heights, currentRoute?.file, activeType, resolvedView?.id])
  const rowHeightSaveQueue = useRef(Promise.resolve())
  const tableOnRowHeightsChange = useCallback((heights: Record<string, number>) => {
    if (!currentRoute || !activeType || !resolvedView) return
    const file = currentRoute.file, type = activeType, view = resolvedView.id
    setProjectSettings(current => current ? { ...current, table_row_heights: {
      ...current.table_row_heights, [file]: { ...current.table_row_heights[file], [type]: {
        ...current.table_row_heights[file]?.[type], [view]: heights,
      } },
    } } : current)
    const identity = generation.currentIdentity()
    if (!api.isTauri || !identity) return
    // 连续拖动串行持久化，防止旧请求覆盖最终行高。
    rowHeightSaveQueue.current = rowHeightSaveQueue.current.then(async () => {
      if (generation.currentSession() !== identity.sessionId) return
      await api.setTableRowHeights(identity.sessionId, file, type, view, heights)
    }).catch(error => { if (generation.currentSession() === identity.sessionId) setErrorMsg(`保存行高失败：${errorMessage(error)}`) })
  }, [currentRoute?.file, activeType, resolvedView?.id, generation, setProjectSettings])
  const tableOnExpandCell = useCallback((coordinate: RecordCoordinate, path: FieldPathSegment[] | null, origin: MotionOrigin) => {
    if (!currentRoute) return
    if (deferDataNavigation(() => tableOnExpandCell(coordinate, path, origin))) return
    setInspectorSelection(path ? valueSelection(currentRoute.file, coordinate, path) : recordSelection(currentRoute.file, coordinate))
    setInspectorOrigin(origin)
    setInspectorDialogOpen(true)
  }, [currentRoute?.file])
  const tableOnColumnWidthsChange = useCallback((widths: Record<string, number>) => {
    if (!api.isTauri || currentRoute?.view !== 'table' || !activeType || !resolvedView) return
    const identity = generation.currentIdentity()
    if (!identity) return
    // Default table view widths live in default_table_column_widths; custom
    // table views carry their own widths inside the ViewConfig.
    const save = resolvedView.isDefault
      ? api.setDefaultTableColumnWidths(identity.sessionId, currentRoute.file, activeType, widths)
      : api.setViewColumnWidths(identity.sessionId, currentRoute.file, activeType, resolvedView.id, widths)
    save
      .then(settings => {
        if (generation.isCurrent(identity.sessionId, identity.revision)) setProjectSettings(settings)
      })
      .catch(err => {
        if (generation.isCurrent(identity.sessionId, identity.revision)) {
          setErrorMsg(`保存列宽失败: ${errorMessage(err)}`)
        }
      })
  }, [activeType, currentRoute?.file, currentRoute?.view, resolvedView, generation])
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
    (coordinate: RecordCoordinate, newId: string): Promise<RecordRow | void> => {
      if (currentRoute?.view === 'table') return renameRecord(currentRoute.file, coordinate, newId)
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
  const tableOnDeleteRecords = useCallback(
    (coordinates: readonly RecordCoordinate[]): Promise<void> => {
      if (currentRoute?.view === 'table') return deleteRecords(currentRoute.file, coordinates)
      return Promise.resolve()
    },
    [currentRoute?.view, currentRoute?.file, deleteRecords],
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
      focusDiagnosticForAnchor(currentRoute.file, coordinate.id, coordinate.actual_type, fieldPath)
    },
    [currentRoute?.view, currentRoute?.file, focusDiagnosticForAnchor],
  )

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

  function switchView(tab: ViewTab): void {
    preloadView(tab.kind)
    if (deferSourceNavigation(() => switchView(tab))) return
    if (!currentRoute) return
    if (tab.kind === 'table') {
      setFirstRecordFocusRequest(0)
      closeInspector()
    }
    const firstCoordinate = tab.kind === 'record'
      ? (
        (activeType
          ? activeFileData?.records.find(r => recordActualType(r) === activeType)
          : activeFileData?.records[0])?.coordinate
        ?? activeFileData?.records[0]?.coordinate
      )
      : undefined
    if (tab.kind === 'record' && !firstCoordinate) return

    const id = workspaceTabId(currentRoute.file, activeType)
    pluginDefaultPendingTabsRef.current.delete(id)
    const existing = workspaceTabsRef.current.find(candidate => candidate.id === id)
      ?? defaultWorkspaceTab(currentRoute.file, activeType)
    const nextTab = workspaceTabWithView(existing, tab.kind, tab.id, firstCoordinate)
    const nextTabs = workspaceTabsRef.current.some(candidate => candidate.id === id)
      ? workspaceTabsRef.current.map(candidate => candidate.id === id ? nextTab : candidate)
      : [...workspaceTabsRef.current, nextTab]
    workspaceTabsRef.current = nextTabs
    setWorkspaceTabs(nextTabs)
    router.replace(routeForWorkspaceTab(nextTab, firstCoordinate))
  }

  function switchPluginView(key: string): void {
    if (deferSourceNavigation(() => switchPluginView(key))) return
    if (!currentRoute || !activeFile || !activeType) return
    const view = pluginViews.find(candidate => candidate.key === key)
    if (!view) return
    closeInspector()
    const id = workspaceTabId(activeFile, activeType)
    pluginDefaultPendingTabsRef.current.delete(id)
    const existing = workspaceTabsRef.current.find(candidate => candidate.id === id)
      ?? defaultWorkspaceTab(activeFile, activeType)
    const nextTab: WorkspaceTab = { ...existing, viewKind: 'table', viewId: view.key }
    const nextTabs = workspaceTabsRef.current.some(candidate => candidate.id === id)
      ? workspaceTabsRef.current.map(candidate => candidate.id === id ? nextTab : candidate)
      : [...workspaceTabsRef.current, nextTab]
    workspaceTabsRef.current = nextTabs
    setWorkspaceTabs(nextTabs)
    router.replace({ view: 'table', file: activeFile, viewId: view.key, typeFilter: activeType })
  }

  // Record tabs can be restored before their file records have loaded. Once
  // data arrives, replace the placeholder coordinate with a real record.
  // 仅记录视图需要补齐记录坐标。
  useEffect(() => {
    if (!pluginsReady || !currentRoute || activePluginView) return
    if (activeFileData?.file_path !== currentRoute.file) return
    const activeTab = workspaceTabsRef.current.find(tab => tab.id === activeWorkspaceTabId)
    const needsRecord = activeTab?.viewKind === 'record'
    if (!needsRecord) return
    const coordinateIsValid = currentRoute.view === 'record'
      && activeFileData.records.some(record => (
        sameCoordinate(record.coordinate, currentRoute.coordinate)
        && (!activeType || recordActualType(record) === activeType)
      ))
    if (coordinateIsValid) return
    const firstCoord = activeFileData.records.find(
      record => !activeType || recordActualType(record) === activeType,
    )?.coordinate
    if (firstCoord) {
      router.replace({
        view: 'record',
        file: currentRoute.file,
        viewId: DEFAULT_RECORD_VIEW_ID,
        coordinate: firstCoord,
      })
    }
  }, [activeFileData, activePluginView, activeType, activeWorkspaceTabId, currentRoute, pluginsReady, router])

  // Deleted custom views and stale routes fall back locally for this tab.
  useEffect(() => {
    if (!pluginsReady || !currentRoute || !activeFileData || viewTabs.length === 0) return
    const valid = !!activePluginView
      || viewTabs.some(tab => tab.id === currentRoute.viewId && tab.kind === currentRoute.view)
    if (valid) return
    if (!activeType) {
      router.replace({
        view: 'source',
        file: currentRoute.file,
        viewId: DEFAULT_SOURCE_VIEW_ID,
        typeFilter: '',
      })
      return
    }
    router.replace({
      view: 'table',
      file: currentRoute.file,
      viewId: DEFAULT_TABLE_VIEW_ID,
      typeFilter: activeType,
    })
  }, [activeFileData, activePluginView, activeType, currentRoute, pluginsReady, router, viewTabs])

  // If the current file doesn't support graph view but a stale route asks for
  // it, drop back to the type's valid default view.
  useEffect(() => {
    if (!currentRoute || currentRoute.view !== 'graph') return
    if (activeFileData?.file_path !== currentRoute.file) return
    if (graphSupported) return
    router.replace({ view: 'table', file: currentRoute.file, viewId: DEFAULT_TABLE_VIEW_ID, typeFilter: activeType })
  }, [currentRoute, activeFileData, graphSupported, activeType, router])

  function activateDocumentTab(id: string): void {
    if (deferSourceNavigation(() => activateDocumentTab(id))) return
    if (id === GIT_DIFF_TAB_ID) {
      finishActiveDataEdit()
      afterSourceSave(() => setGitDiffActive(true))
      setActivePluginPageKey(null)
      setActivePane('changes')
      return
    }
    const pluginTab = pluginPageTabs.find(tab => tab.key === id)
    if (pluginTab) {
      finishActiveDataEdit()
      setGitDiffActive(false)
      setActivePluginPageKey(pluginTab.key)
      return
    }
    const workspaceTab = workspaceTabs.find(tab => tab.id === id)
    if (workspaceTab) openFile(workspaceTab.filePath, workspaceTab.typeName)
  }

  function editView(viewId: string) {
    const saved = projectSettings?.views[activeFile ?? '']?.[activeType]?.find(view => view.id === viewId)
    if (saved) setViewEditor({ mode: 'edit', view: saved })
    else if (viewId === DEFAULT_TABLE_VIEW_ID) setViewEditor({ mode: 'edit', view: {
      id: DEFAULT_TABLE_VIEW_ID, name: '表格', kind: 'table', columns: viewEditorFields,
      column_widths: {}, relations: [], fields: [], group_filter: null,
    } })
  }

  const renderViewPicker = () => <ViewPicker tabs={viewTabs} activeId={currentRoute?.viewId}
    plugins={pluginViews} activePluginKey={activePluginView?.key} onSelect={switchView} onPreview={tab => preloadView(tab.kind)} onSelectPlugin={switchPluginView}
    onCreate={activeType && !isSingletonType ? openViewEditor : undefined}
    onDelete={viewId => void deleteView(viewId)}
    onEdit={editView}
    onReorder={ids => {
      if (activeFile) saveViewOrder(activeFile, activeType, ids)
    }} />

  const renderRecordSearch = () => {
    if (!currentRoute || !activeFileData) return null
    return <>
      <ViewSearchField inputRef={documentSearchRef} placeholder="筛选记录…" label="筛选当前类型"
        value={documentSearch} onChange={setDocumentSearch} onKeyDown={e => {
          if (e.key === 'ArrowDown') { e.preventDefault(); focusFirstRecord() }
          else if (e.key === 'ArrowLeft' && e.currentTarget.selectionStart === 0) { e.preventDefault(); focusFileTree() }
        }}>
      <button className="key-only-toggle" aria-label="只搜索键" aria-pressed={keyOnlySearch}
        title={keyOnlySearch ? '只搜索键：已开启' : '只搜索键：未开启，当前搜索所有字段（含嵌套）'}
        onClick={() => setKeyOnlySearch(value => !value)}><Icon name="key" size={14} /></button>
      </ViewSearchField>

      <span
        className="global-search-count"
        title={documentSearch ? `匹配 ${matchedCount} 条 / 共 ${typeCount} 条` : `共 ${typeCount} 条`}
      >
        {matchedCount} / {typeCount}
      </span>
      {(
        <div className="record-edit-actions">
          <button className="btn btn-primary btn-icon" title="新增记录" aria-label="新增记录"
            disabled={readOnly || !activeFileData.capabilities.can_insert_record || !activeType}
            onClick={() => void createRecordFromTree(currentRoute.file, activeType)}>
            <Icon name="plus" size={16} />
          </button>
        </div>
      )}
  </>
  }

  const sourceQuery = sourceSearch.file === currentRoute?.file ? sourceSearch.query : ''
  const sourceStep = sourceSearch.file === currentRoute?.file ? sourceSearch.step : 0
  const sourceStatus = sourceSearchStatus.file === currentRoute?.file ? sourceSearchStatus : { current: 0, total: 0 }
  const changeSourceQuery = (query: string) => setSourceSearch({ file: currentRoute?.file ?? '', query, step: 0 })
  const moveSourceMatch = (direction: number) => setSourceSearch({ file: currentRoute?.file ?? '', query: sourceQuery, step: sourceStep + direction })
  const renderSourceSearch = () => <>
    <ViewSearchField inputRef={documentSearchRef} label="搜索本文件文本" placeholder="搜索本文件文本…"
      value={sourceQuery} onChange={changeSourceQuery} onKeyDown={event => {
        if (event.key === 'Enter') { event.preventDefault(); moveSourceMatch(event.shiftKey ? -1 : 1) }
        else if (event.key === 'Escape') { event.preventDefault(); changeSourceQuery('') }
      }} />
    <span className="global-search-count" aria-live="polite">{sourceQuery ? sourceStatus.current : 0} / {sourceQuery ? sourceStatus.total : 0}</span>
    <div className="source-search-actions">
      <button className="btn btn-icon" aria-label="上一个匹配" title="上一个匹配（Shift+Enter）" disabled={!sourceQuery || !sourceStatus.total} onClick={() => moveSourceMatch(-1)}><Icon name="arrow-left" size={14} /></button>
      <button className="btn btn-icon" aria-label="下一个匹配" title="下一个匹配（Enter）" disabled={!sourceQuery || !sourceStatus.total} onClick={() => moveSourceMatch(1)}><Icon name="chevron-right" size={14} /></button>
    </div>
  </>

  return (
    <ShortNameContext.Provider value={{ fields: projectSettings?.short_name_fields ?? {}, setField: shortNameSaving ? undefined : saveShortNameField }}>
    <ObjectDraftHost
      lookups={lookups}
      generationKey={`${lookupGenerationKey}:${shortNameRevision}`}
      sessionId={project?.session_id}
      onOpenReference={openReference}
      onLookupError={key => setDiagFocus(previous => ({ key, tick: (previous?.tick ?? 0) + 1 }))}
    >
    <div className={`app${navigationCollapsed ? ' navigation-collapsed' : ''}`}
>


      {projectSearchContribution && projectSearchContext && <ProjectSearchDialog open={projectSearchOpen} onClose={() => setProjectSearchOpen(false)} contribution={projectSearchContribution} context={projectSearchContext} />}
      <div
        className="topbar"
        role="toolbar"
        aria-label="编辑器工具栏"
        onKeyDown={event => onToolbarKeyDown(event, focusFileTree)}
      >
        <div className="topbar-left">
          <span className="app-title">
            {/* SVG 可能被内联为含引号的 data URL，CSS URL 必须显式包裹。 */}
            <span className="app-logo" style={{ maskImage: `url("${coflowLogo}")` }} aria-hidden />
            coflow
          </span>
            <Menu className="project-menu" title="项目" label={<><Icon name="folder" size={14} /><span>{project ? (api.isTauri ? project.project_root.split(/[\\/]/).filter(Boolean).pop() : '示例项目') : '打开项目'}</span><Icon name="chevron-down" size={12} /></>}>
                <button className="btn" onClick={openProject}><Icon name="open" size={14} />打开项目</button>
                <button className="btn" onClick={newProject}><Icon name="plus" size={14} />新建工程</button>
            </Menu>
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
          {(
            <button className="project-search-trigger" title="搜索记录、Key 或字段值 (Ctrl+Shift+F)" aria-label="搜索项目"
              disabled={!pluginRegistry.sidebars.some(sidebar => sidebar.pluginId === 'project-search')}
              onClick={() => {
                setProjectSearchOpen(true)
              }}>
              <Icon name="search" size={14} />
              <span>搜索</span>
              <kbd>Ctrl ⇧ F</kbd>
            </button>
          )}
        </div>
        <div className="topbar-right">
          {project && (
            <button className="btn btn-icon build-action" onClick={runBuild} disabled={!api.isTauri || projectAction !== null}
              aria-label={buildPending ? '构建项目，有待生成的变更' : '构建项目'}
              title={!api.isTauri ? '构建仅在桌面编辑器中可用' : buildPending ? '生成内容已变更，需要重新构建' : '构建项目'}>
              <Icon name={projectAction === 'build' ? 'refresh' : 'build'} size={17} className={projectAction === 'build' ? 'icon-spin' : undefined} />
              {buildPending && <span className="build-pending-indicator" aria-hidden />}
            </button>
          )}
          <EditorSettingsMenu theme={theme} onToggleTheme={toggleTheme} onShowHelp={() => setShowHelp(true)} />
          <button className="btn btn-icon" title={`撤销 (Ctrl+Z) · ${historySnapshot.undo.length}`} aria-label="撤销"
            disabled={historySnapshot.undo.length === 0} onClick={() => { void undo() }}><Icon name="undo" size={16} /></button>
          <button className="btn btn-icon" title={`重做 (Ctrl+Y) · ${historySnapshot.redo.length}`} aria-label="重做"
            disabled={historySnapshot.redo.length === 0} onClick={() => { void redo() }}><Icon name="redo" size={16} /></button>
        </div>
      </div>

      {(errorMsg || recoveryRequired) && (
        <div className="error-banner" role="alert">
          <Icon name="error" size={13} />
          {errorMsg ?? '项目需要重新加载后才能继续写入'}
          {recoveryRequired && <button className="btn" onClick={() => { void recoverProject() }}>重新加载项目</button>}
          {!recoveryRequired && <button className="btn btn-icon" onClick={() => setErrorMsg(null)} aria-label="关闭错误提示">
            <Icon name="close" size={12} />
          </button>}
        </div>
      )}

      <div className="main-layout">
        {/* 导航入口和资源面板共享宽度，收起时保留图标入口。 */}
        <div className={`layout-navigation${splitterDragging ? ' resizing' : ''}`}>
        <div className="navigation-header">
        <ActivityBar
          activePane={activePane}
          registry={pluginRegistry}
          onSelectPane={pane => {
            setActivePane(pane)
            setNavigationCollapsed(false)
          }}
          onSelectFiles={() => {
            setNavigationCollapsed(false)
            setActivePane('files')
            focusFileTree()
          }}
          onSelectChanges={() => {
            setNavigationCollapsed(false)
            finishActiveDataEdit()
            setActivePane('changes')
            setGitDiffOpen(true)
            afterSourceSave(() => setGitDiffActive(true))
            setActivePluginPageKey(null)
          }}
        />


          <button className="btn navigation-toggle" aria-label={navigationCollapsed ? '展开侧栏' : '折叠侧栏'}
            title={navigationCollapsed ? '展开侧栏' : '折叠侧栏'} aria-expanded={!navigationCollapsed}
            onClick={() => setNavigationCollapsed(value => !value)}>
            <Icon name={navigationCollapsed ? "panel-left-closed" : "panel-left"} size={17} />
          </button>
        </div>
        <div className="sidebar" ref={sidebarRef}>
          {activePane === 'files' && (
            <>
              {project ? (
                <FileTree
                  nodes={project.file_tree}
                  dimensions={projectDimensions}
                  fileTypes={navigationFileTypes}
                  selectedDimension={activeDimensionTarget ?? null}
                  onSelectDimension={openDimension}
                  selectedFile={activeDimensionTarget ? null : activeFile}
                  selectedType={activeType}
                  onSelectFile={openFile}
                  onExitRight={focusFirstRecord}
                  onOpenSourceFile={openSourceFile}
                  onAddInput={addProjectInput}
                  onCreateRecord={createRecordFromTree}
                  onCreateFile={createFileFromTree}
                  onDeleteEntry={deleteEntryFromTree}
                />
              ) : (
                <div className="sidebar-empty">
                  {api.isTauri ? '未打开项目' : '浏览器预览（Mock）'}
                </div>
              )}
            </>
          )}
          {activePane === 'plugins' && (
            <div className="plugins-pane">
              <div className="sidebar-header plugins-header">
                <span>插件</span>
                {api.isTauri && (
                  <button className="btn btn-icon" onClick={() => void loadPluginFromSettings()} disabled={pluginLoadBusy || !project} title="添加项目插件" aria-label="添加项目插件">
                    <Icon name="plus" size={15} />
                  </button>
                )}
              </div>
              {pluginRegistry.pages.length > 0 && (
                <div className="plugin-pages-list" aria-label="插件页面">
                  {pluginRegistry.pages.map(page => (
                    <button key={page.key} type="button" onClick={() => openPluginPage(page.key)}>
                      <Icon name="extensions" size={14} aria-hidden />
                      <span>{page.title}</span>
                    </button>
                  ))}
                </div>
              )}
              <div className="plugins-list">
                {pluginSettings.map(plugin => (
                  <article className="plugin-item" key={plugin.id}>
                    <div className="plugin-item-main">
                      <div className="plugin-item-icon"><Icon name="extensions" size={16} /></div>
                      <div>
                        <strong>{plugin.name}</strong>
                        <small>{plugin.description || plugin.id}</small>
                        <em>{plugin.origin === 'project' ? '项目插件' : '全局已安装'}</em>
                      </div>
                    </div>
                    <div className="plugin-item-actions">
                      <label className="plugin-toggle">
                        <input type="checkbox" checked={plugin.enabled} onChange={event => void togglePluginFromSettings(plugin, event.target.checked)} />
                        <span>{plugin.enabled ? '已启用' : '已禁用'}</span>
                      </label>
                      {(plugin.origin === 'project' || plugin.origin === 'global') && (
                        <button className="btn btn-icon" title={plugin.origin === 'project' ? '从项目移除插件' : '卸载全局插件'} aria-label={`卸载 ${plugin.name}`} onClick={() => void uninstallPluginFromSettings(plugin)}>
                          <Icon name="close" size={13} />
                        </button>
                      )}
                    </div>
                  </article>
                ))}
                {pluginLoadError && <div className="plugins-error">{pluginLoadError}</div>}
              </div>
            </div>
          )}
          {activePane === 'changes' && (
            <Suspense fallback={<ViewLoading />}><GitDiffSidebar
              nodes={project?.file_tree ?? []}
              dimensions={projectDimensions}
              fileTypes={project?.file_types ?? {}}
              diff={projectDiff}
              loading={projectDiffLoading}
              error={projectDiffError}
              selection={gitDiffSelection}
              onSelectionChange={selection => {
                setGitDiffSelection(selection)
                setGitDiffOpen(true)
                afterSourceSave(() => setGitDiffActive(true))
                setActivePluginPageKey(null)
              }}
              onRefresh={() => void loadProjectDiff()}
            /></Suspense>
          )}
          {activePluginSidebar && pluginSidebarContext && (
            <PluginContributionMount
              contribution={activePluginSidebar}
              context={pluginSidebarContext}
              className="plugin-sidebar-host"
            />
          )}
        </div>
        {api.isTauri && <UpdateControl />}
        </div>

        <div
          className={`sidebar-splitter${splitterDragging ? ' dragging' : ''}`}
          onMouseDown={onSplitterMouseDown}
          role="separator"
          aria-orientation="vertical"
          aria-label="调整侧栏宽度"
          tabIndex={0}
          onKeyDown={e => {
            if (e.key === 'ArrowLeft') resizeSidebar(-16)
            if (e.key === 'ArrowRight') resizeSidebar(16)
          }}
        />

        <div className="editor-column">
        <div className="content-area-wrap">
        <div className="content-area">
          <DocumentTabs
            fileTypes={project?.file_types}
            dimensions={projectDimensions}
            workspaceTabs={workspaceTabs}
            activeWorkspaceTabId={activeWorkspaceTabId}
            pluginTabs={pluginPageTabs}
            activePluginTabKey={activePluginPageKey}
            gitDiffOpen={gitDiffOpen}
            gitDiffActive={gitDiffActive}
            activeWorkspaceReadOnly={readOnly}
            onActivate={activateDocumentTab}
            onCloseWorkspace={closeWorkspaceTab}
            onClosePlugin={closePluginPageTab}
            onCloseGitDiff={() => {
              setGitDiffOpen(false)
              setGitDiffActive(false)
            }}
          />
          {!gitDiffActive && !activePluginPage && currentRoute && (activeSchemaFile || activeFileData || activeDimensionData) && (
            activeSchemaFile ? (
              <ViewToolbar className="source-search-toolbar">
                <BuiltinViewPicker views={['source']} active="source" onSelect={() => {}} />
                {renderSourceSearch()}
              </ViewToolbar>
            ) : activeDimensionData ? null
            : activeFileData ? (
              // 文件内各视图共用同一工具栏节点，切换源码不会重建标签和中断选中底色。
              <ViewToolbar className={activeViewKind === 'plugin' ? 'view-toolbar' : ''}>
                {renderViewPicker()}
                {activeViewKind === 'source' ? renderSourceSearch() : activeViewKind !== 'plugin' && renderRecordSearch()}
              </ViewToolbar>
            ) : null
          )}
          <Suspense fallback={<ViewLoading />}>
          {gitDiffActive ? (
            <div className="view-container">
              <GitDiffMode
                sessionId={project?.session_id ?? null}
                fileTypes={project?.file_types ?? {}}
                diff={projectDiff}
                loading={projectDiffLoading}
                error={projectDiffError}
                selection={gitDiffSelection}
                onSelectionChange={setGitDiffSelection}
                onRefresh={() => void loadProjectDiff()}
              />
            </div>
          ) : activePluginPage && pluginPageContext ? (
            <div className="view-container plugin-page-container">
              <PluginContributionMount
                contribution={activePluginPage}
                context={pluginPageContext}
                className="plugin-page-host"
                retainOnError
                fallback={<div className="content-empty"><div className="content-empty-title">插件页面加载失败</div></div>}
              />
            </div>
          ) : currentRoute && (activeSchemaFile || activeViewKind === 'source') && project ? (
            <div className="view-container" ref={viewContainerRef}>
              <SourceEditorView
                key={`${project.session_id}:${currentRoute.file}`}
                sessionId={project.session_id}
                revision={project.revision}
                filePath={currentRoute.file}
                searchQuery={sourceQuery}
                searchStep={sourceStep}
                onSearchStatus={(current, total) => setSourceSearchStatus({ file: currentRoute.file, current, total })}
                readOnly={activeSchemaFile ? recoveryRequired : readOnly}
                onSaved={refreshFromBootstrap}
                focus={sourceDiagnosticFocus?.file === currentRoute.file ? sourceDiagnosticFocus : null}
              />
            </div>
          ) : currentRoute && selectedDimensionData ? (
            <DimensionTableView
              onModeChange={setDimensionView}
              key={activeWorkspaceTabId}
              data={selectedDimensionData}
              mode={dimensionView}
              recordGroupsByFile={projectSettings?.record_groups}
              onRenameGroup={renameDimensionRecordGroup}
              onColorGroup={colorDimensionRecordGroup}
              onWrite={(row, variant, expected, next) =>
                writeDimensionCell(activeDimensionData!, row, variant, expected, next)}
              onRenderCellText={tableOnRenderCellText}
              onParseCellText={tableOnParseCellText}
              onExitLeft={focusFileTree}
              onExitUp={focusDocumentTabs}
              focusRequest={firstRecordFocusRequest}
              onFocusRequestConsumed={consumeFirstRecordFocusRequest}
            />
          ) : currentRoute && activeFileData ? (
            <>
              {(readOnly || activePluginView?.readOnly) && (
                <div className="document-toolbar readonly-only">
                  <span className="document-readonly" title="该来源未提供可写能力">
                    <Icon name="lock" size={11} aria-hidden />
                    只读
                  </span>
                </div>
              )}

              {/* Record search bar — shared by structured views. */}
              <div className="structured-view-content">
              <div className="structured-view-main">


              <div className="view-container" ref={viewContainerRef}>
                {activeViewKind === 'plugin' && activePluginView && pluginViewContext && (
                  <PluginContributionMount
                    contribution={activePluginView}
                    context={pluginViewContext}
                    className="plugin-view-host"
                  />
                )}
                {activeViewKind === 'table' && (
                  <TableView
                    data={viewFilteredFileData ?? activeFileData}
                    activeType={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    searchQuery={documentSearch}
                    keyOnlySearch={keyOnlySearch}
                    fullTextSearch={true}
                    recordGroups={resolvedView?.groupFilter ? [] : recordGroups}
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
                    onValueSelectionCellsChange={tableOnValueSelectionCellsChange}
                    onRenderCellText={tableOnRenderCellText}
                    onParseCellText={tableOnParseCellText}
                    onClearSelection={clearWorkspaceSelection}
                    onOpenRecord={tableOnOpenRecord}
                    onWriteField={tableOnWriteField}
                    onWriteFieldBatch={tableOnWriteFieldBatch}
                    onRenameRecord={tableOnRenameRecord}
                    onInsertRecord={tableOnInsertRecord}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDeleteRecords={tableOnDeleteRecords}
                    onMoveRecord={tableOnMoveRecord}
                    onDiagnosticBadgeClick={tableOnBadgeClick}
                    rowHeights={tableRowHeights}
                    onRowHeightsChange={tableOnRowHeightsChange}
                    onExpandCell={tableOnExpandCell}
                    columnWidths={tableColumnWidths}
                    onColumnWidthsChange={tableOnColumnWidthsChange}
                    visibleColumns={resolvedView?.kind === 'table' ? resolvedView.columns : undefined}
                    onEnterInspector={tableOnEnterInspector}
                    focusRequest={tableFocusRequest}
                    firstRecordFocusRequest={firstRecordFocusRequest}
                    onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                    onNavigationBoundary={direction => {
                      if (direction === 'ArrowLeft') focusFileTree()
                      else if (direction === 'ArrowUp') focusDocumentSearch()
                    }}
                  />
                )}
                {activeViewKind === 'record' && activeRecordCoordinate && (
                  documentSearch.trim() && matchedCount === 0 ? (
                    <div className="empty-hint">无匹配 "{documentSearch}" 的记录</div>
                  ) : <RecordView
                    data={activeFileData}
                    coordinate={activeRecordCoordinate}
                    typeFilter={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    recordSearch={documentSearch}
                    fullTextSearch={true}
                    keyOnlySearch={keyOnlySearch}
                    hideRecordList={isSingletonType}
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
                      if (selectionMatchesValue(inspectorSelection, currentRoute.file, coordinate, path)) return
                      finishActiveDataEdit()
                      setInspectorSelection(valueSelection(currentRoute.file, coordinate, path))
                    }}
                    onRenderCellText={tableOnRenderCellText}
                    onParseCellText={tableOnParseCellText}
                    onWriteField={(coordinate, path, val) => writeField(currentRoute.file, coordinate, path, val)}
                    onWriteFields={(coordinates, path, val) => writeFields(currentRoute.file, coordinates, path, val)}
                    onCollectionEdit={(coordinate, path, edit) => editCollection(currentRoute.file, coordinate, path, edit)}
                    onRenameRecord={(coordinate, newId) => renameRecord(currentRoute.file, coordinate, newId)}
                    onInsertRecord={(rk, type, fields) => insertRecord(currentRoute.file, rk, type, fields)}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDeleteRecords={(coordinates) => deleteRecords(currentRoute.file, coordinates)}
                    onMoveRecord={(coordinate, targetIndex) =>
                      moveRecord(currentRoute.file, coordinate, targetIndex)}
                    onDiagnosticBadgeClick={(coordinate, fieldPath) =>
                      focusDiagnosticForAnchor(currentRoute.file, coordinate.id, coordinate.actual_type, fieldPath)
                    }
                    onExitLeft={focusFileTree}
                    onExitUp={focusDocumentSearch}
                    firstRecordFocusRequest={firstRecordFocusRequest}
                    onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                  />
                )}
                {activeViewKind === 'graph' && (
                  activeGraph ? (
                    <GraphView
                      key={`${currentRoute.file}:${resolvedView?.id ?? currentRoute.viewId}:${activeType}`}
                      viewKey={graphViewKey(currentRoute.file, currentRoute.viewId, activeType)}
                      savedPositions={projectSettings?.graph_positions[graphViewKey(currentRoute.file, currentRoute.viewId, activeType)] as GraphPositions | undefined}
                      onSavePositions={(positions, recordHistory) => saveGraphPositions(
                        graphViewKey(currentRoute.file, currentRoute.viewId, activeType), positions, recordHistory)}
                      savedCompact={projectSettings?.graph_compact_modes[graphViewKey(currentRoute.file, currentRoute.viewId, activeType)]}
                      onSaveCompact={compact => {
                        void saveGraphCompactMode(graphViewKey(currentRoute.file, currentRoute.viewId, activeType), compact)
                      }}
                      graphData={viewFilteredGraph ?? activeGraph}
                      filePath={currentRoute.file}
                      activeType={activeType}
                      enabledFieldsOverride={resolvedView?.kind === 'graph' && !resolvedView.isDefault ? resolvedView.relations : undefined}
                      visibleCardFields={visibleFields}
                      fileCapabilities={fileCapabilities}
                      diagnostics={project?.diagnostics}
                      onOpenRecord={(file, coordinate) => openRecord(file, coordinate)}
                      onSelectRecord={openInspector}
                      onClearSelection={clearWorkspaceSelection}
                      selectedCoordinate={inspectorCoord}
                      onWriteField={writeField}
                      onCollectionEdit={editCollection}
                      onDiagnosticBadgeClick={(file, coordinate, fieldPath) =>
                        focusDiagnosticForAnchor(file, coordinate.id, coordinate.actual_type, fieldPath)
                      }
                      onCreateRecordDraft={tableOnCreateRecordDraft}
                      onInsertRecord={(recordId, actualType, fields) =>
                        insertRecord(currentRoute.file, recordId, actualType, fields)}
                      recordGroups={recordGroups}
                      onDropRecordIntoGroup={dropRecordIntoGroup}
                      onRenameRecord={renameRecord}
                      onDeleteRecord={(file, coordinate) => deleteRecords(file, [coordinate])}
                      runHistoryBatch={runHistoryBatch}
                      onExitLeft={focusFileTree}
                      onExitUp={focusDocumentSearch}
                      onExitRight={focusInspector}
                      firstRecordFocusRequest={firstRecordFocusRequest}
                      onFirstRecordFocusConsumed={consumeFirstRecordFocusRequest}
                    />
                  ) : (
                    <div className="empty-hint">加载图谱中…</div>
                  )
                )}

              </div>
              </div>
        <InspectorPanel
          modal
          origin={inspectorOrigin}
          open={inspectorDialogOpen && !!inspectorSelection && !!activeFile && !gitDiffActive && !activePluginPage
            && (activeViewKind === 'table' || activeViewKind === 'graph')
            && !activeDimensionTarget}
          data={inspectorCoord ? fileDataCache[inspectorCoord.file] ?? null : null}
          selection={inspectorSelection}
          valueCells={inspectorValueCells}
          readOnly={inspectorCoord ? !isEditableFile(fileDataCache[inspectorCoord.file]) : true}
          diagnostics={project?.diagnostics}
          width={inspectorW}
          onWidthChange={setInspectorW}
          onClose={closeInspector}
          onWriteField={writeField}
          onWriteFields={writeFields}
          onWriteFieldBatch={tableOnWriteFieldBatch}
          onRenderCellText={(_filePath, coordinate, path) => tableOnRenderCellText(coordinate, path)}
          onParseCellText={(_filePath, coordinate, path, text) => tableOnParseCellText(coordinate, path, text)}
          onCollectionEdit={editCollection}
          onRenameRecord={renameRecord}
          onDiagnosticBadgeClick={(coordinate, fieldPath) => {
            if (!inspectorCoord) return
            focusDiagnosticForAnchor(inspectorCoord.file, coordinate.id, coordinate.actual_type, fieldPath)
          }}
          focusRequest={inspectorFocusRequest}
          onExitKeyboardNavigation={inspectorOnExitKeyboardNavigation}
          visibleFields={activeViewKind === 'graph' ? undefined : visibleFields}
        />
              </div>
            </>
          ) : dataQueries.fileQuery.isFetching || dataQueries.dimensionQuery.isFetching ? (
            <div className="content-empty">
              <div className="content-empty-title">加载 {dataQueries.file} 中…</div>
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
          </Suspense>
        </div>

        </div>
        {project && (
          <DiagnosticsPanel
            initialCollapsed={true}
            diagnostics={[...project.diagnostics, ...lookupDiagnostics]}
            currentFile={activeFile ?? undefined}
            focus={diagFocus}
            onFocusConsumed={() => setDiagFocus(null)}
            onJump={jumpToDiagnostic}
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

      {viewEditor && activeFile && activeType && (
        <ViewEditorDialog
          initial={viewEditor.mode === 'edit' ? viewEditor.view : null}
          availableFields={viewEditorFields}
          availableRelations={viewEditorRelations}
          groups={recordGroups}
          onSubmit={submitViewConfig}
          onClose={() => setViewEditor(null)}
        />
      )}

      {treeRecordDraft && (
        <CreateRecordDialog
          actualType={treeRecordDraft.actualType}
          typeOptions={treeRecordDraft.data.type_names}
          existingIds={treeRecordDraft.data.records.map(record => record.coordinate.id)}
          onCreateRecordDraft={tableOnCreateRecordDraft}
          onInsertRecord={async (key, type, fields) => {
            await insertRecord(treeRecordDraft.filePath, key, type, fields)
          }}
          onClose={() => setTreeRecordDraft(null)}
        />
      )}
      {fileActionDialog?.kind === 'create' && (
        <TextInputDialog
          title="新建文件"
          message={`在 ${fileActionDialog.parentPath} 中创建文件`}
          placeholder="文件名"
          suffix={fileActionDialog.sourceKind === 'schema' ? '.cft' : '.cfd'}
          confirmLabel="创建"
          busy={fileActionBusy}
          onClose={() => setFileActionDialog(null)}
          onConfirm={async entered => {
            const identity = generation.currentIdentity()
            if (!identity) return
            const extension = fileActionDialog.sourceKind === 'schema' ? '.cft' : '.cfd'
            const baseName = entered.toLowerCase().endsWith(extension)
              ? entered.slice(0, -extension.length)
              : entered
            const fileName = `${baseName}${extension}`
            if (!await flushSourceChanges()) return
            setFileActionBusy(true)
            try {
              const bootstrap = await api.createProjectFile(identity.sessionId, fileActionDialog.sourceKind, fileActionDialog.parentPath, fileName)
              await refreshFromBootstrap(bootstrap)
              setFileActionDialog(null)
            } catch (error) {
              structureError(identity.sessionId, '新建文件失败', error)
            } finally {
              setFileActionBusy(false)
            }
          }}
        />
      )}
      {fileActionDialog?.kind === 'delete' && (
        <ConfirmDialog
          title="删除文件"
          message={`确认递归删除“${fileActionDialog.path}”？此操作不可撤销。`}
          confirmLabel="删除"
          danger
          busy={fileActionBusy}
          onClose={() => setFileActionDialog(null)}
          onConfirm={async () => {
            const identity = generation.currentIdentity()
            if (!identity) return
            if (!await flushSourceChanges()) return
            setFileActionBusy(true)
            try {
              const bootstrap = await api.deleteProjectEntry(identity.sessionId, fileActionDialog.path)
              setTreeRecordDraft(null)
              await refreshFromBootstrap(bootstrap)
              setFileActionDialog(null)
            } catch (error) {
              structureError(identity.sessionId, '删除失败', error)
            } finally {
              setFileActionBusy(false)
            }
          }}
        />
      )}
      <HelpDialog open={showHelp} onClose={() => setShowHelp(false)} />
    </div>
      {referenceTarget && <ReferencePanel
        recordId={referenceTarget.coordinate.id}
        actualType={referenceTarget.coordinate.actual_type}
        title={referenceTarget.short_name ?? referenceTarget.coordinate.id}
        depth={referenceHistory.length}
        onBack={() => setReferenceHistory(current => current.slice(0, -1))}
        onClose={() => setReferenceHistory([])}
        onNavigate={() => { navigateReference(referenceTarget.coordinate.actual_type, referenceTarget.coordinate.id); setReferenceHistory([]) }}>
        {referenceError ? <div role="alert" className="content-empty">{referenceError}</div> : <InspectorPanel
          open embedded data={fileDataCache[referenceTarget.file_path] ?? null}
          selection={recordSelection(referenceTarget.file_path, referenceTarget.coordinate)}
          readOnly={!isEditableFile(fileDataCache[referenceTarget.file_path])}
          width={520} onWidthChange={() => {}} onClose={() => setReferenceHistory([])}
          diagnostics={project?.diagnostics} onWriteField={writeField} onWriteFields={writeFields}
          onWriteFieldBatch={writes => mutations.writeFieldBatch(referenceTarget.file_path, writes)}
          onRenderCellText={(_file, coordinate, path) => tableOnRenderCellText(coordinate, path)}
          onParseCellText={(_file, coordinate, path, text) => tableOnParseCellText(coordinate, path, text)}
          onCollectionEdit={editCollection} />}
      </ReferencePanel>}
    </ObjectDraftHost>
    </ShortNameContext.Provider>
  )
}
