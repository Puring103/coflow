import { useState, useEffect, useCallback, useMemo, useRef, useSyncExternalStore } from 'react'
import { FileTree } from './components/FileTree'
import { TableView } from './components/TableView'
import { RecordView } from './components/RecordView'
import { GraphView } from './components/GraphView'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { InspectorPanel } from './components/InspectorPanel'
import { Icon } from './components/Icon'
import { ObjectDraftHost } from './components/ObjectDraftHost'
import { useRouter } from './hooks/useRouter'
import { useTheme } from './hooks/useTheme'
import { MOCK_PROJECT, MOCK_FILE_RECORDS, MOCK_GRAPH } from './mock'
import * as api from './api'
import type { FileRecords } from './bindings/FileRecords'
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
  recordActualType,
  recordKey,
  sameCoordinate,
  type FieldPathSegment,
  type FieldValue,
} from './wire'
import { recordMatchesSearch } from './value/fieldValue'
import { typeColor } from './utils/typeColor'
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
import './style.css'

const GRAPH_DEPTH = 3
const GRAPH_LIMIT = 1_000

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

export default function App() {
  const [project, setProject] = useState<ProjectSnapshot | null>(null)
  const [generation] = useState(() => new ProjectGenerationController())
  const [history] = useState(() => new MutationHistoryController())
  const [lookups] = useState(() => new EditorLookupController(api))
  const lookupGenerationKey = project ? `${project.session_id}:${project.revision}` : 'none'
  const historySnapshot = useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot)
  const [fileDataCache, setFileDataCache] = useState<Record<string, FileRecords>>({})
  const [graphCache, setGraphCache] = useState<Record<string, GraphData>>({})
  const [showHelp, setShowHelp] = useState(false)
  const helpBoxRef = useRef<HTMLDivElement>(null)
  const helpReturnRef = useRef<HTMLElement | null>(null)
  const [loadingFile, setLoadingFile] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  const router = useRouter()
  const { theme, toggle: toggleTheme } = useTheme()
  const [activeType, setActiveType] = useState<string>('')
  // The last view the user actively picked. `openFile` pushes a table
  // placeholder because record view needs a coordinate we don't yet have;
  // once the file data lands, the effect below upgrades the route to
  // `preferredView` if that's not what we currently show.
  const [preferredView, setPreferredView] = useState<'table' | 'record' | 'graph'>('table')
  const [globalSearch, setGlobalSearch] = useState('')
  const globalSearchRef = useRef<HTMLInputElement>(null)
  const [inspectorCollapsed, setInspectorCollapsed] = useState(false)
  // Field path to briefly highlight after a diagnostic jump. Cleared after
  // the RecordView applies the highlight so subsequent navigations don't
  // re-flash it.
  const [highlightField, setHighlightField] = useState<string | null>(null)
  // Diagnostics panel focus: which item (by stable key) should be revealed
  // and pulsed. Set from either the panel itself (self-scroll) or from a
  // record/field corner badge click. Consumed by DiagnosticsPanel; we bump
  // `diagFocusTick` so repeat clicks on the same badge re-flash the item.
  const [diagFocus, setDiagFocus] = useState<{ key: string; tick: number } | null>(null)

  // Resizable sidebar width, persisted to localStorage.
  const [sidebarW, setSidebarW] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('cfd-editor-sidebar-w') : null
    const n = raw ? parseInt(raw, 10) : NaN
    return Number.isFinite(n) ? Math.min(480, Math.max(160, n)) : 220
  })
  const [splitterDragging, setSplitterDragging] = useState(false)

  // Right-side inspector panel: shared between table and graph views. Selection
  // lives here so switching views keeps the same record highlighted. Overlays
  // content area without pushing it. Width persisted to localStorage.
  const [inspectorCoord, setInspectorCoord] = useState<{ file: string; coordinate: RecordCoordinate } | null>(null)
  const inspectorOpen = inspectorCoord !== null
  const [inspectorW, setInspectorW] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('cfd-editor-inspector-w') : null
    const n = raw ? parseInt(raw, 10) : NaN
    return Number.isFinite(n) ? Math.min(720, Math.max(320, n)) : 420
  })
  useEffect(() => {
    try { localStorage.setItem('cfd-editor-inspector-w', String(inspectorW)) } catch { /* quota */ }
  }, [inspectorW])
  const openInspector = useCallback((file: string, coordinate: RecordCoordinate) => {
    setInspectorCoord(prev => {
      if (prev && prev.file === file && sameCoordinate(prev.coordinate, coordinate)) return prev
      return { file, coordinate }
    })
  }, [])
  const closeInspector = useCallback(() => setInspectorCoord(null), [])

  // Auto-load mock data only when not running in Tauri (browser preview).
  useEffect(() => {
    if (!api.isTauri) {
      generation.adopt(MOCK_PROJECT)
      lookups.adopt({ sessionId: MOCK_PROJECT.session_id, revision: MOCK_PROJECT.revision })
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setGraphCache({ [graphCacheKey('data/npc.cfd', GRAPH_DEPTH, GRAPH_LIMIT)]: MOCK_GRAPH })
      if (MOCK_PROJECT.first_source_file) {
        router.push({ view: 'table', file: MOCK_PROJECT.first_source_file })
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
      setGraphCache({})
      history.clear()
      const firstFile = snapshot.first_source_file ?? collectSourceFiles(snapshot)[0]
      if (firstFile) router.push({ view: 'table', file: firstFile })
    },
    [generation, history, lookups, router]
  )

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
      return
    }
    const request = generation.beginProjectRequest()
    const yamlPath = await api.pickProjectYaml()
    if (!generation.isProjectRequestCurrent(request) || !yamlPath) return
    setErrorMsg(null)
    try {
      const snapshot = await api.loadProject(yamlPath)
      if (!generation.isProjectRequestCurrent(request)) return
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
        router.push({ view: 'table', file: nextFile })
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
          router.replace(stillExists ? current : { view: 'table', file: nextFile })
        } else {
          router.replace(current)
        }
      } catch (err) {
        if (generation.isCurrent(snapshot.session_id, snapshot.revision)) {
          setProject(snapshot)
          reportSessionError(snapshot.session_id, '刷新项目失败', err)
          router.push({ view: 'table', file: nextFile })
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
  }, [generation, project, router.current, fileDataCache, reportSessionError])

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
    (filePath: string) => {
      setGlobalSearch('')
      // Preserve the current view mode so the user doesn't get bounced back
      // to table on every file click. Record view needs a coordinate — push
      // table for now; the effect below promotes the route to record view
      // (with the first record's coordinate) as soon as file data lands.
      const currentView = router.current?.view ?? 'table'
      if (currentView === 'graph') {
        router.push({ view: 'graph', file: filePath })
        return
      }
      router.push({ view: 'table', file: filePath })
    },
    [router]
  )

  const openRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate) => {
      setPreferredView('record')
      // Keep the type tab in sync with the record the user is opening, so
      // the record-view sidebar (filtered by activeType) actually contains
      // it and the tab highlight matches.
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
    (oldCoordinate: RecordCoordinate, newCoordinate: RecordCoordinate) => {
      if (sameCoordinate(oldCoordinate, newCoordinate)) return
      if (
        router.current?.view === 'record' &&
        sameCoordinate(router.current.coordinate, oldCoordinate)
      ) {
        router.replace({ ...router.current, coordinate: newCoordinate })
      }
      setInspectorCoord(current => (
        current && sameCoordinate(current.coordinate, oldCoordinate)
          ? { ...current, coordinate: newCoordinate }
          : current
      ))
    },
    [router],
  )

  const mutationPort = useMemo<EditorMutationPort>(() => ({
    currentGeneration: () => api.isTauri ? generation.currentIdentity() : null,
    publish: publishMutation,
    rebindCoordinate,
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
  }), [commitProjectRevision, generation, publishMutation, rebindCoordinate, reportSessionError])
  const mutations = useMemo(
    () => new EditorMutationController(api, mutationPort, history),
    [history, mutationPort],
  )

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
      return mutations.renameRecord(filePath, coordinate, newKey)
    },
    [mutations],
  )

  const insertRecord = useCallback(
    async (filePath: string, recordKey: string, actualType: string, fields: FieldValue) => {
      await mutations.insertRecord(filePath, recordKey, actualType, fields)
    },
    [mutations],
  )
  const deleteRecord = useCallback(
    async (filePath: string, coordinate: RecordCoordinate) => {
      await mutations.deleteRecord(filePath, coordinate)
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
      // Undo / redo. Skip when typing in a text control so native input
      // undo stays available there.
      if ((e.metaKey || e.ctrlKey) && (e.key === 'z' || e.key === 'Z') && !isTextTarget(e.target)) {
        e.preventDefault()
        if (e.shiftKey) redo()
        else undo()
      }
      if ((e.metaKey || e.ctrlKey) && e.key === 'y' && !isTextTarget(e.target)) {
        e.preventDefault()
        redo()
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
  const activeFileData = activeFile ? fileDataCache[activeFile] : null
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
  const fileDiagnostics = useMemo(
    () => activeFile && project ? project.diagnostics.filter(d => d.file_path === activeFile) : [],
    [activeFile, project?.diagnostics],
  )
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

  // Stable callbacks for TableView so React.memo can bail out on re-renders
  // caused by inspector panel state changes (collapsed, open, width).
  const tableOnSelectRecord = useCallback(
    (coordinate: RecordCoordinate) => {
      if (currentRoute?.view === 'table') openInspector(currentRoute.file, coordinate)
    },
    [currentRoute?.view, currentRoute?.file, openInspector],
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

  // Sync activeType when file or its type set changes
  useEffect(() => {
    if (!activeFileData) return
    if (!activeFileData.type_names.includes(activeType)) {
      setActiveType(activeFileData.type_names[0] ?? '')
    }
  }, [activeFileData?.file_path, activeFileData?.type_names])

  function switchView(view: 'table' | 'record' | 'graph') {
    if (!currentRoute) return
    setPreferredView(view)
    if (view === 'record') {
      const firstCoordinate =
        (activeType
          ? activeFileData?.records.find(r => recordActualType(r) === activeType)
          : activeFileData?.records[0])?.coordinate
        ?? activeFileData?.records[0]?.coordinate
      if (!firstCoordinate) return
      router.replace({ view, file: currentRoute.file, coordinate: firstCoordinate })
    } else {
      router.replace({ view, file: currentRoute.file } as typeof currentRoute)
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
      const firstCoord = activeFileData.records[0]?.coordinate
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

  return (
    <ObjectDraftHost lookups={lookups} generationKey={lookupGenerationKey}>
    <div className="app">
      <div className="topbar">
        <span className="app-title">CFD Editor</span>
        <button className="btn btn-outlined" onClick={openProject}>
          <Icon name="open" size={13} />
          打开项目
        </button>
        <button
          className="btn btn-outlined"
          onClick={newProject}
          title="选一个空目录创建新的 Coflow 工程（等价于 coflow init）"
        >
          <Icon name="plus" size={13} />
          新建工程
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
        {project && (
          <span className="project-root" title={project.project_root}>
            {project.project_root}
          </span>
        )}
        <span className="topbar-spacer" />
        {(historySnapshot.undo.length > 0 || historySnapshot.redo.length > 0) && (
          <span className="undo-badge" title={`可撤销 ${historySnapshot.undo.length} 步 / 可重做 ${historySnapshot.redo.length} 步 (Ctrl+Z / Ctrl+Y)`}>
            {historySnapshot.undo.length > 0 ? `可撤销 ${historySnapshot.undo.length}` : `可重做 ${historySnapshot.redo.length}`}
          </span>
        )}
        <button
          className="btn btn-icon"
          onClick={toggleTheme}
          title={theme === 'dark' ? '切换到浅色模式' : '切换到深色模式'}
          aria-label={theme === 'dark' ? '切换到浅色模式' : '切换到深色模式'}
        >
          <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={14} />
        </button>
        <button
          className="btn btn-icon"
          onClick={() => setShowHelp(v => !v)}
          title="帮助 (?)"
          aria-label="帮助"
        >
          <Icon name="help" size={14} />
        </button>
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
        <div className="sidebar">
          <div className="sidebar-header">
            <span>文件</span>
          </div>
          {project ? (
            <FileTree
              nodes={project.file_tree}
              selectedFile={activeFile}
              onSelectFile={openFile}
            />
          ) : (
            <div className="sidebar-empty">
              {api.isTauri ? '未打开项目' : '浏览器预览（Mock）'}
            </div>
          )}
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

        <div className="content-area-wrap">
        <div className="content-area">
          {currentRoute && activeFileData ? (
            <>
              {/* File breadcrumb */}
              <div className="content-breadcrumb">
                <Icon name="file" size={12} className="breadcrumb-icon" aria-hidden />
                {activeFile?.split('/').map((part, i, arr) => {
                  const dirPath = arr.slice(0, i + 1).join('/')
                  const isLeaf = i === arr.length - 1
                  const siblingFile = firstSourceFileForPath(project, dirPath)
                  const clickable = !isLeaf && !!siblingFile
                  return (
                    <span key={i} className="breadcrumb-part">
                      {i > 0 && <span className="breadcrumb-sep" aria-hidden>/</span>}
                      {clickable && siblingFile ? (
                        <button
                          type="button"
                          className="breadcrumb-link"
                          title={`跳转到 ${siblingFile}`}
                          onClick={() => openFile(siblingFile)}
                        >
                          {part}
                        </button>
                      ) : (
                        <span className={isLeaf ? 'breadcrumb-leaf' : ''}>{part}</span>
                      )}
                    </span>
                  )
                })}
                {readOnly && (
                  <span className="breadcrumb-readonly" title="该来源未提供可写能力">
                    <Icon name="lock" size={11} aria-hidden />
                    只读
                  </span>
                )}
              </div>

              {/* Type tabs row */}
              {activeFileData.type_names.length > 0 && (
                <div className="view-tabs view-tabs-types" role="tablist" aria-label="类型">
                  <div className="type-tabs-inline">
                    {activeFileData.type_names.map(t => (
                      <button
                        key={t}
                        className={`tab-btn${activeType === t ? ' active' : ''}`}
                        role="tab"
                        aria-selected={activeType === t}
                        tabIndex={activeType === t ? 0 : -1}
                        data-tab-id={t}
                        onClick={() => setActiveType(t)}
                        onKeyDown={e => onTabListKeyDown(e, activeFileData.type_names, setActiveType)}
                        style={{'--tab-color': typeColor(t), '--tab-color-dim': typeColor(t)} as React.CSSProperties}
                        >
                          {t}
                          <span className="tab-count">
                          {activeFileData.records.filter(r => recordActualType(r) === t).length}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* View switcher */}
              <div className="view-tabs view-tabs-views" role="tablist" aria-label="视图">
                {(['record', 'table', 'graph'] as const).map(v => (
                  <button
                    key={v}
                    className={`tab-btn tab-view${currentRoute.view === v ? ' active' : ''}`}
                    role="tab"
                    aria-selected={currentRoute.view === v}
                    tabIndex={currentRoute.view === v ? 0 : -1}
                    data-tab-id={v}
                    onClick={() => switchView(v)}
                    onKeyDown={e => onTabListKeyDown(e, ['record', 'table', 'graph'], v => switchView(v as 'table' | 'record' | 'graph'))}
                  >
                    <Icon name={v === 'table' ? 'table' : v === 'record' ? 'record' : 'graph'} size={13} aria-hidden />
                    {v === 'table' ? '表格' : v === 'record' ? '记录' : '图谱'}
                  </button>
                ))}
              </div>

              {/* Record search bar — shared across all three views */}
              <div className="global-search-bar">
                <Icon name="search" size={13} className="global-search-icon" aria-hidden />
                <input
                  ref={globalSearchRef}
                  placeholder="搜索记录… (Ctrl+F)"
                  value={globalSearch}
                  onChange={e => setGlobalSearch(e.target.value)}
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

              <div className="view-container">
                {currentRoute.view === 'table' && (
                  <TableView
                    data={activeFileData}
                    activeType={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    searchQuery={globalSearch}
                    selectedCoordinate={
                      inspectorCoord && inspectorCoord.file === currentRoute.file
                        ? inspectorCoord.coordinate
                        : null
                    }
                    onSelectRecord={tableOnSelectRecord}
                    onClearSelection={closeInspector}
                    onOpenRecord={tableOnOpenRecord}
                    onWriteField={tableOnWriteField}
                    onRenameRecord={tableOnRenameRecord}
                    onInsertRecord={tableOnInsertRecord}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDeleteRecord={tableOnDeleteRecord}
                    onDiagnosticBadgeClick={tableOnBadgeClick}
                  />
                )}
                {currentRoute.view === 'record' && (
                  <RecordView
                    data={activeFileData}
                    coordinate={currentRoute.coordinate}
                    typeFilter={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    recordSearch={globalSearch}
                    highlightField={highlightField}
                    onHighlightConsumed={() => setHighlightField(null)}
                    onOpenRecord={coordinate => openRecord(currentRoute.file, coordinate)}
                    onWriteField={(coordinate, path, val) => writeField(currentRoute.file, coordinate, path, val)}
                    onCollectionEdit={(coordinate, path, edit) => editCollection(currentRoute.file, coordinate, path, edit)}
                    onRenameRecord={(coordinate, newKey) => renameRecord(currentRoute.file, coordinate, newKey)}
                    onInsertRecord={(rk, type, fields) => insertRecord(currentRoute.file, rk, type, fields)}
                    onCreateRecordDraft={tableOnCreateRecordDraft}
                    onDiagnosticBadgeClick={(coordinate, fieldPath) =>
                      focusDiagnosticForAnchor(currentRoute.file, coordinate.key, coordinate.actual_type, fieldPath)
                    }
                  />
                )}
                {currentRoute.view === 'graph' && (
                  activeGraph ? (
                    <GraphView
                      graphData={activeGraph}
                      activeType={activeType}
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
          open={inspectorOpen || ((currentRoute?.view === 'table' || currentRoute?.view === 'graph') && !!activeFileData)}
          collapsed={inspectorCollapsed}
          onToggleCollapse={() => setInspectorCollapsed(v => !v)}
          data={inspectorCoord ? fileDataCache[inspectorCoord.file] ?? null : null}
          coordinate={inspectorCoord?.coordinate ?? null}
          readOnly={inspectorCoord ? !isEditableFile(fileDataCache[inspectorCoord.file]) : true}
          diagnostics={project?.diagnostics}
          width={inspectorW}
          onWidthChange={setInspectorW}
          onClose={closeInspector}
          onWriteField={writeField}
          onCollectionEdit={editCollection}
          onRenameRecord={renameRecord}
          onDiagnosticBadgeClick={(coordinate, fieldPath) => {
            if (!inspectorCoord) return
            focusDiagnosticForAnchor(inspectorCoord.file, coordinate.key, coordinate.actual_type, fieldPath)
          }}
        />
        </div>
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

function collectSourceFiles(snapshot: ProjectSnapshot): string[] {
  const out: string[] = []
  function walk(n: ProjectSnapshot['file_tree'][number]) {
    if (!n.is_dir && n.in_sources) out.push(n.path)
    for (const c of n.children) walk(c)
  }
  for (const n of snapshot.file_tree) walk(n)
  return out
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
) {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Home' && e.key !== 'End') return
  const nodes = Array.from(
    e.currentTarget.parentElement?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [],
  )
  const i = nodes.indexOf(e.currentTarget as HTMLElement)
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    e.preventDefault()
    const dir = e.key === 'ArrowRight' ? 1 : -1
    const next = nodes[(i + dir + nodes.length) % nodes.length]
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

/** Find the first in-source file whose path starts with `dirPath/`. Used to
 *  make breadcrumb path segments clickable to jump into that directory. */
function firstSourceFileForPath(project: ProjectSnapshot | null, dirPath: string): string | null {
  if (!project) return null
  function find(n: ProjectSnapshot['file_tree'][number]): string | null {
    if (n.path === dirPath) return n.first_source_descendant ?? null
    for (const c of n.children) {
      const hit = find(c)
      if (hit) return hit
    }
    return null
  }
  for (const n of project.file_tree) {
    const hit = find(n)
    if (hit) return hit
  }
  return null
}
