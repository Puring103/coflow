import { useState, useEffect, useCallback, useRef } from 'react'
import { FileTree } from './components/FileTree'
import { TableView } from './components/TableView'
import { RecordView } from './components/RecordView'
import { GraphView } from './components/GraphView'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { Icon } from './components/Icon'
import { useRouter } from './hooks/useRouter'
import { useTheme } from './hooks/useTheme'
import { MOCK_PROJECT, MOCK_FILE_RECORDS, MOCK_GRAPH } from './mock'
import * as api from './api'
import type { ProjectSnapshot, FileRecords, GraphData, FieldValue, FieldPathSegment, DiagnosticItem, RecordRow } from './bindings/index'
import { errorMessage, errorDiagnostics } from './bindings/index'
import type { FieldDiagnostic } from './components/DataCard'
import { typeColor } from './utils/typeColor'
import { isEditableFile } from './utils/editable'
import { setActiveSession } from './utils/editContext'
import './style.css'

export default function App() {
  const [project, setProject] = useState<ProjectSnapshot | null>(null)
  const [fileDataCache, setFileDataCache] = useState<Record<string, FileRecords>>({})
  const [graphCache, setGraphCache] = useState<Record<string, GraphData>>({})
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
  // Field path to briefly highlight after a diagnostic jump. Cleared after
  // the RecordView applies the highlight so subsequent navigations don't
  // re-flash it.
  const [highlightField, setHighlightField] = useState<string | null>(null)

  // Auto-load mock data only when not running in Tauri (browser preview).
  useEffect(() => {
    if (!api.isTauri) {
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      setGraphCache({ 'data/npc.cfd': MOCK_GRAPH })
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

  // Lazy-load graph when switching to graph view
  useEffect(() => {
    if (!project || router.current?.view !== 'graph') return
    const file = router.current.file
    if (graphCache[file]) return
    if (!api.isTauri) return
    api
      .getGraph(project.session_id, file)
      .then(g => setGraphCache(c => ({ ...c, [file]: g })))
      .catch(err => setErrorMsg(`读取图谱失败: ${errorMessage(err)}`))
  }, [project, router.current, graphCache])

  const openFile = useCallback(
    (filePath: string) => {
      router.push({ view: 'table', file: filePath })
    },
    [router]
  )

  const openRecord = useCallback(
    (filePath: string, recordKey: string) => {
      router.push({ view: 'record', file: filePath, recordKey })
    },
    [router]
  )

  // Core write pipeline shared by user edits, undo, and redo.
  // `opts.recordHistory` controls whether the edit is pushed onto the undo
  // stack (redo intentionally replays without re-recording the inverse, so
  // it passes the oldValue it is reverting to as the "new" value).
  const writeFieldInternal = useCallback(
    async (
      filePath: string,
      recordKey: string,
      fieldPath: FieldPathSegment[],
      newValue: FieldValue,
      opts: { recordHistory: boolean; oldValue?: FieldValue } = { recordHistory: true },
    ): Promise<RecordRow | void> => {
      if (!project || !api.isTauri) return
      const mySeq = ++writeSeqRef.current
      try {
        const outcome = await api.writeField(
          project.session_id,
          filePath,
          recordKey,
          fieldPath,
          newValue,
        )
        // Stale guard: a newer edit superseded this one; drop our refresh so
        // we don't clobber the newer state with older data.
        if (mySeq !== writeSeqRef.current) return outcome.row
        setProject(p => (p ? { ...p, diagnostics: outcome.diagnostics } : p))
        const refreshed = await api.getFileRecords(project.session_id, filePath)
        if (mySeq !== writeSeqRef.current) return outcome.row
        setFileDataCache({ [filePath]: refreshed })
        setGraphCache({})
        if (opts.recordHistory) {
          const oldValue = opts.oldValue ?? snapshotOldValue(fileDataCache, filePath, recordKey, fieldPath)
          if (oldValue) {
            setUndoStack(s => [...s, {
              filePath, recordKey, fieldPath,
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
    [project, fileDataCache],
  )

  const writeField = useCallback(
    (filePath: string, recordKey: string, fieldPath: FieldPathSegment[], newValue: FieldValue) =>
      writeFieldInternal(filePath, recordKey, fieldPath, newValue),
    [writeFieldInternal],
  )

  const undo = useCallback(async () => {
    const entry = undoStack[undoStack.length - 1]
    if (!entry) return
    setUndoStack(s => s.slice(0, -1))
    setRedoStack(s => [...s, entry])
    await writeFieldInternal(entry.filePath, entry.recordKey, entry.fieldPath, entry.oldValue, {
      recordHistory: false,
    })
  }, [undoStack, writeFieldInternal])

  const redo = useCallback(async () => {
    const entry = redoStack[redoStack.length - 1]
    if (!entry) return
    setRedoStack(s => s.slice(0, -1))
    setUndoStack(s => [...s, entry])
    await writeFieldInternal(entry.filePath, entry.recordKey, entry.fieldPath, entry.newValue, {
      recordHistory: false,
    })
  }, [redoStack, writeFieldInternal])

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
  const activeGraph = activeFile ? graphCache[activeFile] : null
  const readOnly = !isEditableFile(activeFileData)
  const fileDiagnostics = activeFile && project
    ? project.diagnostics.filter(d => d.file_path === activeFile)
    : []

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
      const firstKey = activeFileData?.records[0]?.key
      if (!firstKey) return
      router.replace({ view, file: currentRoute.file, recordKey: firstKey })
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
                  <span className="breadcrumb-readonly" title="非 .cfd 源文件，仅可查看">
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
                        style={activeType === t ? {'--tab-color': typeColor(t)} as React.CSSProperties : undefined}
                      >
                        {t}
                        <span className="tab-count">
                          {activeFileData.records.filter(r => r.actual_type === t).length}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* View switcher */}
              <div className="view-tabs view-tabs-views" role="tablist" aria-label="视图">
                {(['table', 'record', 'graph'] as const).map(v => (
                  <button
                    key={v}
                    className={`tab-btn tab-view${currentRoute.view === v ? ' active' : ''}`}
                    role="tab"
                    aria-selected={currentRoute.view === v}
                    tabIndex={currentRoute.view === v ? 0 : -1}
                    data-tab-id={v}
                    onClick={() => switchView(v)}
                    onKeyDown={e => onTabListKeyDown(e, ['table', 'record', 'graph'], v => switchView(v as 'table' | 'record' | 'graph'))}
                  >
                    <Icon name={v === 'table' ? 'table' : v === 'record' ? 'record' : 'graph'} size={13} aria-hidden />
                    {v === 'table' ? '表格' : v === 'record' ? '记录' : '图谱'}
                  </button>
                ))}
              </div>

              <div className="view-container">
                {currentRoute.view === 'table' && (
                  <TableView
                    data={activeFileData}
                    activeType={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    onOpenRecord={key => openRecord(currentRoute.file, key)}
                    onWriteField={(rk, path, val) => writeField(currentRoute.file, rk, path, val)}
                  />
                )}
                {currentRoute.view === 'record' && (
                  <RecordView
                    data={activeFileData}
                    recordKey={currentRoute.recordKey}
                    typeFilter={activeType}
                    readOnly={readOnly}
                    diagnostics={fileDiagnostics}
                    highlightField={highlightField}
                    onHighlightConsumed={() => setHighlightField(null)}
                    onOpenRecord={key => openRecord(currentRoute.file, key)}
                    onWriteField={(rk, path, val) => writeField(currentRoute.file, rk, path, val)}
                  />
                )}
                {currentRoute.view === 'graph' && (
                  activeGraph ? (
                    <GraphView
                      graphData={activeGraph}
                      activeType={activeType}
                      onOpenRecord={(file, key) => openRecord(file, key)}
                      onWriteField={writeField}
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
      </div>

      {project && (
        <DiagnosticsPanel
          diagnostics={project.diagnostics}
          onJumpToRecord={(file, key) => openRecord(file, key)}
          onJumpToField={(file, key, fieldPath) => {
            setHighlightField(fieldPath)
            openRecord(file, key)
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
  recordKey: string,
): FieldDiagnostic[] {
  const out: FieldDiagnostic[] = []
  for (const d of diags) {
    if (d.file_path !== filePath) continue
    if (d.record_key !== recordKey) continue
    if (!d.field_path) continue
    out.push({ severity: d.severity, fieldPath: d.field_path, message: d.message })
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

/** A single reversible field edit. `oldValue` is the pre-edit FieldValue so
 *  undo replays it back through writeField. We deep-clone via structuredClone
 *  so later mutations of the cache can't retroactively rewrite history. */
interface EditEntry {
  filePath: string
  recordKey: string
  fieldPath: FieldPathSegment[]
  oldValue: FieldValue
  newValue: FieldValue
}

/** Walk a FieldValue along a FieldPathSegment path and return the value that
 *  lives there, or null if any segment doesn't resolve. Used to snapshot the
 *  pre-edit value of an arbitrary nested field. */
function readFieldPath(root: FieldValue, path: FieldPathSegment[]): FieldValue | null {
  let cur: FieldValue = root
  for (const seg of path) {
    if (seg.kind === 'field') {
      if (cur.kind !== 'Object') return null
      const fc = cur.fields.find(f => f.name === seg.name)
      if (!fc) return null
      cur = fc.value
    } else {
      if (cur.kind !== 'Array') return null
      const item = cur.items[seg.i]
      if (!item) return null
      cur = item
    }
  }
  return cur
}

function cloneValue(v: FieldValue): FieldValue {
  if (typeof structuredClone === 'function') return structuredClone(v)
  return JSON.parse(JSON.stringify(v)) as FieldValue
}

/** Look up the current value at (filePath, recordKey, fieldPath) in the
 *  file-data cache so we can record it as the undo target. Returns null if
 *  the file/record/path isn't present (e.g. cache miss). */
function snapshotOldValue(
  cache: Record<string, FileRecords>,
  filePath: string,
  recordKey: string,
  fieldPath: FieldPathSegment[],
): FieldValue | null {
  const fr = cache[filePath]
  if (!fr) return null
  const row = fr.records.find(r => r.key === recordKey)
  if (!row) return null
  const root: FieldValue = { kind: 'Object', actual_type: row.actual_type, fields: row.fields }
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
