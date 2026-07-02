import { useState, useEffect, useCallback, useMemo, useRef } from 'react'
import { FileTree } from './components/FileTree'
import { TableView } from './components/TableView'
import { RecordView } from './components/RecordView'
import { GraphView } from './components/GraphView'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { InspectorPanel } from './components/InspectorPanel'
import { Icon } from './components/Icon'
import { useRouter } from './hooks/useRouter'
import { useTheme } from './hooks/useTheme'
import { MOCK_PROJECT, MOCK_FILE_RECORDS, MOCK_GRAPH } from './mock'
import * as api from './api'
import type { FileRecords } from './bindings/FileRecords'
import type { GraphData } from './bindings/GraphData'
import type { ProjectSnapshot } from './bindings/ProjectSnapshot'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import type { RecordRow } from './bindings/RecordRow'
import type { WriterCapabilities } from './bindings/WriterCapabilities'
import {
  cloneValue,
  deletedSnapshotValue,
  diagnosticKey,
  diagnosticMatchesAnchor,
  diagnosticMatchesCoordinate,
  diagnosticSeverity,
  errorDiagnostics,
  errorMessage,
  fieldPathField,
  makeObjectValue,
  objectFields,
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from './wire'
import { summaryOf } from './components/DataCard'
import type { FieldDiagnostic } from './components/DataCard'
import { typeColor } from './utils/typeColor'
import { isEditableFile } from './utils/editable'
import { setActiveSession } from './utils/editContext'
import './style.css'

const GRAPH_DEPTH = 3
const GRAPH_LIMIT = 1_000

/** Passed as `highlightField` when a record-level (no field path) jump lands
 *  on a record view — RecordView flashes the CardHeader instead of a row. */
export const RECORD_HIGHLIGHT_SENTINEL = '__record__'

function graphCacheKey(
  filePath: string,
  activeType: string | null | undefined,
  enabledFields: string[] | null | undefined,
  depth: number,
  limit: number,
): string {
  const fields = enabledFields ? enabledFields.join(',') : '*'
  return `${filePath}::${activeType || '*'}::${fields}::${depth}::${limit}`
}

function sameStringList(a: readonly string[] | null, b: readonly string[]): boolean {
  if (!a || a.length !== b.length) return false
  return a.every((item, index) => item === b[index])
}

export default function App() {
  const [project, setProject] = useState<ProjectSnapshot | null>(null)
  const [fileDataCache, setFileDataCache] = useState<Record<string, FileRecords>>({})
  const [graphCache, setGraphCache] = useState<Record<string, GraphData>>({})
  const [graphEnabledFields, setGraphEnabledFields] = useState<string[] | null>(null)
  const [showHelp, setShowHelp] = useState(false)
  const helpBoxRef = useRef<HTMLDivElement>(null)
  const helpReturnRef = useRef<HTMLElement | null>(null)
  const [loadingFile, setLoadingFile] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  // Edit history for undo/redo. Each entry records the pre-edit value so we
  // can replay it backwards through the same writeField pipeline. The future
  // stack is populated by an undo and drained by redo.
  const [undoStack, setUndoStack] = useState<EditEntry[]>([])
  const [redoStack, setRedoStack] = useState<EditEntry[]>([])
  // Monotonic sequence guard so stale write completions can't overwrite a
  // newer edit's refresh (e.g. when two edits race on the same file).
  const writeSeqRef = useRef(0)

  const router = useRouter()
  const { theme, toggle: toggleTheme } = useTheme()
  const [activeType, setActiveType] = useState<string>('')
  const [globalSearch, setGlobalSearch] = useState('')
  const globalSearchRef = useRef<HTMLInputElement>(null)
  const [inspectorCollapsed, setInspectorCollapsed] = useState(false)
  const setGraphEnabledFieldsStable = useCallback((fields: string[]) => {
    const next = Array.from(new Set(fields)).sort()
    setGraphEnabledFields(prev => (
      sameStringList(prev, next) ? prev : next
    ))
  }, [])
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
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setGraphCache({ [graphCacheKey('data/npc.cfd', null, null, GRAPH_DEPTH, GRAPH_LIMIT)]: MOCK_GRAPH })
      const firstFile = MOCK_PROJECT.file_tree
        .flatMap(n => (n.is_dir ? n.children : [n]))
        .find(n => !n.is_dir && n.in_sources)
      if (firstFile) router.push({ view: 'table', file: firstFile.path })
    }
  }, [])

  // Reset all per-session UI state to a clean slate before swapping in a
  // new project snapshot. Used by both "open" and "new" flows so behavior
  // is identical. Also closes the previous backend session so the
  // SessionStore doesn't accumulate stale sessions across project switches.
  const adoptSnapshot = useCallback(
    (snapshot: ProjectSnapshot) => {
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
      setUndoStack([])
      setRedoStack([])
      const firstFile = collectSourceFiles(snapshot)[0]
      if (firstFile) router.push({ view: 'table', file: firstFile })
    },
    [router]
  )

  const openProject = useCallback(async () => {
    if (!api.isTauri) {
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      return
    }
    const yamlPath = await api.pickProjectYaml()
    if (!yamlPath) return
    setErrorMsg(null)
    try {
      const snapshot = await api.loadProject(yamlPath)
      adoptSnapshot(snapshot)
    } catch (err) {
      setErrorMsg(`打开项目失败: ${errorMessage(err)}`)
      const diags = errorDiagnostics(err)
      if (diags.length > 0) {
        setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
      }
    }
  }, [adoptSnapshot])

  const refreshFromSnapshot = useCallback(
    async (snapshot: ProjectSnapshot) => {
      const current = router.current
      const sourceFiles = collectSourceFiles(snapshot)
      const keepFile = current && sourceFiles.includes(current.file)
      const nextFile = keepFile ? current.file : sourceFiles[0]
      setProject(snapshot)
      setFileDataCache({})
      setGraphCache({})
      setUndoStack([])
      setRedoStack([])
      setHighlightField(null)
      writeSeqRef.current += 1
      if (!nextFile) {
        return
      }
      try {
        const fileRecords = api.isTauri
          ? await api.getFileRecords(snapshot.session_id, nextFile)
          : null
        if (fileRecords) {
          setFileDataCache({ [nextFile]: fileRecords })
        }
        if (current && keepFile) {
          if (current.view === 'record') {
            const stillExists = fileRecords?.records.some(r => sameCoordinate(r.coordinate, current.coordinate)) ?? false
            router.replace(stillExists ? current : { view: 'table', file: nextFile })
          } else {
            router.replace(current)
          }
        } else {
          router.push({ view: 'table', file: nextFile })
        }
      } catch (err) {
        setErrorMsg(`刷新项目失败: ${errorMessage(err)}`)
        router.push({ view: 'table', file: nextFile })
      }
    },
    [router],
  )

  useEffect(() => {
    if (!api.isTauri || !project) return
    let disposed = false
    let unlistenChanged: (() => void) | null = null
    let unlistenError: (() => void) | null = null
    api.onProjectChanged(event => {
      if (event.session_id !== project.session_id) return
      refreshFromSnapshot(event.snapshot).catch(err => {
        setErrorMsg(`刷新项目失败: ${errorMessage(err)}`)
      })
    }).then(unlisten => {
      if (disposed) unlisten()
      else unlistenChanged = unlisten
    }).catch(err => setErrorMsg(`监听项目变更失败: ${errorMessage(err)}`))
    api.onProjectWatchError(event => {
      if (event.session_id !== project.session_id) return
      setErrorMsg(`监听项目变更失败: ${event.message}`)
    }).then(unlisten => {
      if (disposed) unlisten()
      else unlistenError = unlisten
    }).catch(err => setErrorMsg(`监听项目变更失败: ${errorMessage(err)}`))
    return () => {
      disposed = true
      unlistenChanged?.()
      unlistenError?.()
    }
  }, [project?.session_id, refreshFromSnapshot])

  // "新建工程": pick an empty directory, scaffold a minimal Coflow
  // project (mirrors `coflow init`), and open it. The same back-end call
  // refuses to clobber an existing `coflow.yaml` and that diagnostic
  // surfaces here as a clear error banner.
  const newProject = useCallback(async () => {
    if (!api.isTauri) {
      setErrorMsg('新建工程仅在桌面环境可用')
      return
    }
    const dir = await api.pickProjectDirectory()
    if (!dir) return
    setErrorMsg(null)
    try {
      const snapshot = await api.initProject(dir)
      adoptSnapshot(snapshot)
    } catch (err) {
      setErrorMsg(`新建工程失败: ${errorMessage(err)}`)
      const diags = errorDiagnostics(err)
      if (diags.length > 0) {
        setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
      }
    }
  }, [adoptSnapshot])

  // Lazy-load file records when navigated to
  useEffect(() => {
    if (!project || !router.current) return
    const file = router.current.file
    if (fileDataCache[file]) return
    if (!api.isTauri) return // mock branch already populated
    setLoadingFile(file)
    api
      .getFileRecords(project.session_id, file)
      .then(records => setFileDataCache(c => ({ ...c, [file]: records })))
      .catch(err => setErrorMsg(`读取文件失败: ${errorMessage(err)}`))
      .finally(() => setLoadingFile(null))
  }, [project, router.current, fileDataCache])

  useEffect(() => {
    setGraphEnabledFields(null)
  }, [router.current?.file, activeType])

  // Lazy-load graph when switching to graph view
  useEffect(() => {
    if (!project || router.current?.view !== 'graph') return
    const file = router.current.file
    const key = graphCacheKey(file, activeType, graphEnabledFields, GRAPH_DEPTH, GRAPH_LIMIT)
    if (graphCache[key]) return
    if (!api.isTauri) {
      setGraphCache(c => ({ ...c, [key]: MOCK_GRAPH }))
      return
    }
    let cancelled = false
    api
      .getGraph(project.session_id, file, {
        activeType,
        enabledFields: graphEnabledFields ?? undefined,
        depth: GRAPH_DEPTH,
        limit: GRAPH_LIMIT,
      })
      .then(g => {
        if (!cancelled) setGraphCache(c => ({ ...c, [key]: g }))
      })
      .catch(err => setErrorMsg(`读取图谱失败: ${errorMessage(err)}`))
    return () => { cancelled = true }
  }, [project, router.current, graphCache, activeType, graphEnabledFields])

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
      router.push({ view: 'table', file: filePath })
    },
    [router]
  )

  const openRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate) => {
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
      api.getFileRecords(project.session_id, filePath)
        .then(records => {
          setFileDataCache(c => ({ ...c, [filePath]: records }))
          const row = records.records.find(r =>
            r.coordinate.key === recordKey && (!actualType || r.coordinate.actual_type === actualType)
          )
          if (row) openRecord(filePath, row.coordinate)
          else setErrorMsg(`记录 ${recordKey} 未找到`)
        })
        .catch(err => setErrorMsg(`读取文件失败: ${errorMessage(err)}`))
    },
    [fileDataCache, openRecord, project],
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
      setUndoStack(stack => stack.map(entry => rebindEntryCoordinate(entry, oldCoordinate, newCoordinate)))
      setRedoStack(stack => stack.map(entry => rebindEntryCoordinate(entry, oldCoordinate, newCoordinate)))
    },
    [router],
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

  // Core write pipeline shared by user edits, undo, and redo.
  // `opts.recordHistory` controls whether the edit is pushed onto the undo
  // stack (redo intentionally replays without re-recording the inverse, so
  // it passes the oldValue it is reverting to as the "new" value).
  const writeFieldInternal = useCallback(
    async (
      filePath: string,
      coordinate: RecordCoordinate,
      fieldPath: FieldPathSegment[],
      newValue: FieldValue,
      opts: { recordHistory: boolean; oldValue?: FieldValue } = { recordHistory: true },
    ): Promise<RecordRow | void> => {
      if (!project || !api.isTauri) return
      const mySeq = ++writeSeqRef.current
      try {
        const outcome = await api.writeField(
          project.session_id,
          coordinate,
          fieldPath,
          newValue,
        )
        // Stale guard: a newer edit superseded this one; drop our refresh so
        // we don't clobber the newer state with older data.
        if (mySeq !== writeSeqRef.current) return outcome.row
        setProject(p => (p ? { ...p, diagnostics: outcome.diagnostics } : p))
        const refreshed = await api.getFileRecords(project.session_id, filePath)
        if (mySeq !== writeSeqRef.current) return outcome.row
        setFileDataCache(c => ({ ...c, [filePath]: refreshed }))
        setGraphCache({})
        const finalCoordinate = outcome.renamed ?? coordinate
        if (outcome.renamed) {
          rebindCoordinate(coordinate, outcome.renamed)
        }
        if (opts.recordHistory) {
          const oldValue = opts.oldValue ?? snapshotOldValue(fileDataCache, filePath, coordinate, fieldPath)
          if (oldValue) {
            setUndoStack(s => [...s, {
              kind: 'field',
              filePath, coordinate: finalCoordinate, fieldPath,
              oldValue: cloneValue(oldValue),
              newValue: cloneValue(newValue),
            }])
            setRedoStack([])
          }
        }
        return outcome.row
      } catch (err) {
        setErrorMsg(`写入失败: ${errorMessage(err)}`)
        // Surface structured diagnostics embedded in a failed write (e.g.
        // type-mismatch detail from the backend) so they land in the
        // diagnostics panel instead of being silently dropped.
        const diags = errorDiagnostics(err)
        if (diags.length > 0) {
          setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
        }
      }
    },
    [project, fileDataCache, rebindCoordinate],
  )

  const writeField = useCallback(
    (filePath: string, coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) =>
      writeFieldInternal(filePath, coordinate, fieldPath, newValue),
    [writeFieldInternal],
  )

  const renameRecordInternal = useCallback(
    async (
      filePath: string,
      coordinate: RecordCoordinate,
      newKey: string,
      opts: { recordHistory: boolean } = { recordHistory: true },
    ): Promise<RecordRow | void> => {
      if (!project || !api.isTauri) return
      const mySeq = ++writeSeqRef.current
      try {
        const outcome = await api.renameRecordKey(project.session_id, coordinate, newKey)
        if (mySeq !== writeSeqRef.current) return outcome.row
        setProject(p => (p ? { ...p, diagnostics: outcome.diagnostics } : p))
        const refreshed = await api.getFileRecords(project.session_id, filePath)
        if (mySeq !== writeSeqRef.current) return outcome.row
        setFileDataCache(c => ({ ...c, [filePath]: refreshed }))
        setGraphCache({})
        rebindCoordinate(coordinate, outcome.renamed)
        if (opts.recordHistory) {
          setUndoStack(s => [...s, {
            kind: 'field',
            filePath,
            coordinate: outcome.renamed,
            fieldPath: [fieldPathField('id')],
            oldValue: { kind: 'string', value: coordinate.key },
            newValue: { kind: 'string', value: newKey },
          }])
          setRedoStack([])
        }
        return outcome.row
      } catch (err) {
        setErrorMsg(`重命名失败: ${errorMessage(err)}`)
        const diags = errorDiagnostics(err)
        if (diags.length > 0) {
          setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
        }
      }
    },
    [project, rebindCoordinate],
  )

  const renameRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate, newKey: string) =>
      renameRecordInternal(filePath, coordinate, newKey),
    [renameRecordInternal],
  )

  // Insert a new record at the top level of `filePath`. The back-end picks
  // the sheet name (for table sources) by reusing an existing record's sheet
  // when one is available, and falls back to provider-specific options
  // otherwise. The returned `FileRecords` is already refreshed — drop it
  // straight into the cache instead of issuing a follow-up `getFileRecords`.
  //
  // `opts.recordHistory` mirrors `writeFieldInternal`: user-initiated inserts
  // push onto the undo stack; replays from undo/redo pass `false` so the
  // history isn't re-entered while we're walking it.
  const insertRecordInternal = useCallback(
    async (
      filePath: string,
      recordKey: string,
      actualType: string,
      fields: FieldValue,
      opts: { recordHistory: boolean } = { recordHistory: true },
    ) => {
      if (!project || !api.isTauri) return
      try {
        const outcome = await api.insertRecord(project.session_id, filePath, recordKey, actualType, fields)
        setProject(p => (p ? { ...p, diagnostics: outcome.diagnostics } : p))
        setFileDataCache(c => ({ ...c, [filePath]: outcome.file_records }))
        setGraphCache({})
        if (opts.recordHistory) {
          setUndoStack(s => [...s, {
            kind: 'insert',
            filePath,
            coordinate: { actual_type: actualType, key: recordKey },
            fields: cloneValue(fields),
          }])
          setRedoStack([])
        }
      } catch (err) {
        setErrorMsg(`新建记录失败: ${errorMessage(err)}`)
        const diags = errorDiagnostics(err)
        if (diags.length > 0) {
          setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
        }
      }
    },
    [project],
  )

  const deleteRecordInternal = useCallback(
    async (
      filePath: string,
      coordinate: RecordCoordinate,
      opts: { recordHistory: boolean } = { recordHistory: true },
    ) => {
      if (!project || !api.isTauri) return
      try {
        const outcome = await api.deleteRecord(project.session_id, coordinate)
        setProject(p => (p ? { ...p, diagnostics: outcome.diagnostics } : p))
        setFileDataCache(c => ({ ...c, [filePath]: outcome.file_records }))
        setGraphCache({})
        // Undo payload comes from the back-end's authoritative snapshot —
        // captured under the same lock as the delete, so it always reflects
        // the engine's view at the moment of deletion (spread/ref metadata
        // included). No front-end cache dependency.
        if (opts.recordHistory && outcome.deleted_snapshot) {
          setUndoStack(s => [...s, {
            kind: 'delete',
            filePath,
            coordinate,
            snapshot: deletedSnapshotValue(outcome.deleted_snapshot!),
          }])
          setRedoStack([])
        }
      } catch (err) {
        setErrorMsg(`删除记录失败: ${errorMessage(err)}`)
        const diags = errorDiagnostics(err)
        if (diags.length > 0) {
          setProject(p => p ? { ...p, diagnostics: [...p.diagnostics, ...diags] } : p)
        }
      }
    },
    [project],
  )

  const insertRecord = useCallback(
    (filePath: string, recordKey: string, actualType: string, fields: FieldValue) =>
      insertRecordInternal(filePath, recordKey, actualType, fields),
    [insertRecordInternal],
  )
  const deleteRecord = useCallback(
    (filePath: string, coordinate: RecordCoordinate) => deleteRecordInternal(filePath, coordinate),
    [deleteRecordInternal],
  )

  const undo = useCallback(async () => {
    const entry = undoStack[undoStack.length - 1]
    if (!entry) return
    setUndoStack(s => s.slice(0, -1))
    setRedoStack(s => [...s, entry])
    if (entry.kind === 'field') {
      await writeFieldInternal(entry.filePath, entry.coordinate, entry.fieldPath, entry.oldValue, {
        recordHistory: false,
      })
    } else if (entry.kind === 'insert') {
      // Invert insert: delete the record we just created. The redo path
      // re-runs the insert from this same entry, so we don't need to
      // capture another snapshot here.
      await deleteRecordInternal(entry.filePath, entry.coordinate, {
        recordHistory: false,
      })
    } else {
      // Invert delete: re-create the record with the captured snapshot.
      await insertRecordInternal(entry.filePath, entry.coordinate.key, entry.coordinate.actual_type, entry.snapshot, {
        recordHistory: false,
      })
    }
  }, [undoStack, writeFieldInternal, insertRecordInternal, deleteRecordInternal])

  const redo = useCallback(async () => {
    const entry = redoStack[redoStack.length - 1]
    if (!entry) return
    setRedoStack(s => s.slice(0, -1))
    setUndoStack(s => [...s, entry])
    if (entry.kind === 'field') {
      await writeFieldInternal(entry.filePath, entry.coordinate, entry.fieldPath, entry.newValue, {
        recordHistory: false,
      })
    } else if (entry.kind === 'insert') {
      await insertRecordInternal(entry.filePath, entry.coordinate.key, entry.coordinate.actual_type, entry.fields, {
        recordHistory: false,
      })
    } else {
      await deleteRecordInternal(entry.filePath, entry.coordinate, {
        recordHistory: false,
      })
    }
  }, [redoStack, writeFieldInternal, insertRecordInternal, deleteRecordInternal])

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
    ? graphCacheKey(activeFile, activeType, graphEnabledFields, GRAPH_DEPTH, GRAPH_LIMIT)
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
    const q = globalSearch.trim().toLowerCase()
    if (!q) return { typeCount: inType.length, matchedCount: inType.length }
    const matched = inType.filter(r => {
      if (recordKey(r).toLowerCase().includes(q)) return true
      for (const f of r.fields) {
        if (f.name.toLowerCase().includes(q)) return true
        if (summaryOf(f.value).toLowerCase().includes(q)) return true
      }
      return false
    })
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

  useEffect(() => {
    setActiveSession(project?.session_id ?? null)
  }, [project?.session_id])

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
    if (view === 'record') {
      const firstCoordinate = activeFileData?.records[0]?.coordinate
      if (!firstCoordinate) return
      router.replace({ view, file: currentRoute.file, coordinate: firstCoordinate })
    } else {
      router.replace({ view, file: currentRoute.file } as typeof currentRoute)
    }
  }

  return (
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
        {(undoStack.length > 0 || redoStack.length > 0) && (
          <span className="undo-badge" title={`可撤销 ${undoStack.length} 步 / 可重做 ${redoStack.length} 步 (Ctrl+Z / Ctrl+Y)`}>
            {undoStack.length > 0 ? `可撤销 ${undoStack.length}` : `可重做 ${redoStack.length}`}
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
                  const siblingFile = findFirstSourceFileInDir(project, dirPath)
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
                    onRenameRecord={(coordinate, newKey) => renameRecord(currentRoute.file, coordinate, newKey)}
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
                      onEnabledFieldsChange={setGraphEnabledFieldsStable}
                      onOpenRecord={(file, coordinate) => openRecord(file, coordinate)}
                      onSelectRecord={openInspector}
                      onClearSelection={closeInspector}
                      selectedCoordinate={inspectorCoord}
                      onWriteField={writeField}
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
  )
}

/** Distill the project's flat diagnostic list down to per-record FieldDiagnostics
 *  for one file. Diagnostics with no field_path are skipped (they apply to the
 *  whole record and surface in the diagnostics panel instead). */
export function diagnosticsForRecord(
  diags: DiagnosticItem[],
  filePath: string,
  coordinate: RecordCoordinate,
): FieldDiagnostic[] {
  const out: FieldDiagnostic[] = []
  for (const d of diags) {
    if (d.file_path !== filePath) continue
    if (!diagnosticMatchesCoordinate(d, coordinate)) continue
    if (!d.field_path) continue
    out.push({ severity: diagnosticSeverity(d.severity), fieldPath: d.field_path, message: d.message })
  }
  return out
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

/** A reversible mutation. Tagged so undo/redo can dispatch to the right API
 *  (field edits replay through `writeField`, record creation/deletion replay
 *  via `insertRecord` / `deleteRecord`). All payloads are deep-cloned via
 *  structuredClone so later cache mutations can't retroactively rewrite
 *  history. */
type EditEntry =
  | FieldEditEntry
  | InsertEditEntry
  | DeleteEditEntry

interface FieldEditEntry {
  kind: 'field'
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  oldValue: FieldValue
  newValue: FieldValue
}

/** Inverted by `deleteRecord(coordinate)`; redone by `insertRecord` with the
 *  same payload. `fields` is the Object FieldValue used at create time so a
 *  redo reproduces the exact same record. */
interface InsertEditEntry {
  kind: 'insert'
  filePath: string
  coordinate: RecordCoordinate
  fields: FieldValue
}

/** Inverted by re-inserting with `snapshot` (captured before deletion so the
 *  full record can be reconstructed); redone by `deleteRecord(coordinate)`. */
interface DeleteEditEntry {
  kind: 'delete'
  filePath: string
  coordinate: RecordCoordinate
  snapshot: FieldValue
}

function rebindEntryCoordinate(
  entry: EditEntry,
  oldCoordinate: RecordCoordinate,
  newCoordinate: RecordCoordinate,
): EditEntry {
  if (!sameCoordinate(entry.coordinate, oldCoordinate)) return entry
  return { ...entry, coordinate: newCoordinate }
}

/** Walk a FieldValue along a FieldPathSegment path and return the value that
 *  lives there, or null if any segment doesn't resolve. Used to snapshot the
 *  pre-edit value of an arbitrary nested field. */
function readFieldPath(root: FieldValue, path: FieldPathSegment[]): FieldValue | null {
  let cur: FieldValue = root
  for (const seg of path) {
    if (seg.kind === 'field') {
      if (cur.kind !== 'object') return null
      const fc = objectFields(cur).find(f => f.name === seg.value)
      if (!fc) return null
      cur = fc.value
    } else if (seg.kind === 'index') {
      if (cur.kind !== 'array') return null
      const item = cur.value[seg.value]
      if (!item) return null
      cur = item
    } else {
      return null
    }
  }
  return cur
}

/** Look up the current value at (filePath, coordinate, fieldPath) in the
 *  file-data cache so we can record it as the undo target. Returns null if
 *  the file/record/path isn't present (e.g. cache miss). */
function snapshotOldValue(
  cache: Record<string, FileRecords>,
  filePath: string,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
): FieldValue | null {
  const fr = cache[filePath]
  if (!fr) return null
  const row = fr.records.find(r => sameCoordinate(r.coordinate, coordinate))
  if (!row) return null
  const root: FieldValue = makeObjectValue(recordActualType(row), row.fields)
  return readFieldPath(root, fieldPath)
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
function findFirstSourceFileInDir(project: ProjectSnapshot | null, dirPath: string): string | null {
  if (!project) return null
  const prefix = dirPath.endsWith('/') ? dirPath : dirPath + '/'
  let result: string | null = null
  function walk(n: ProjectSnapshot['file_tree'][number]) {
    if (result) return
    if (!n.is_dir && n.in_sources && n.path.startsWith(prefix)) {
      result = n.path
      return
    }
    for (const c of n.children) walk(c)
  }
  for (const n of project.file_tree) walk(n)
  return result
}
