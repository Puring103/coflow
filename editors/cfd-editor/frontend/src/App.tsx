import { useState, useEffect, useCallback } from 'react'
import { FileTree } from './components/FileTree'
import { TableView } from './components/TableView'
import { RecordView } from './components/RecordView'
import { GraphView } from './components/GraphView'
import { DiagnosticsPanel } from './components/DiagnosticsPanel'
import { Icon } from './components/Icon'
import { useRouter } from './hooks/useRouter'
import { MOCK_PROJECT, MOCK_FILE_RECORDS, MOCK_GRAPH } from './mock'
import * as api from './api'
import type { ProjectSnapshot, FileRecords, GraphData } from './bindings/index'
import './style.css'

export default function App() {
  const [project, setProject] = useState<ProjectSnapshot | null>(null)
  const [fileDataCache, setFileDataCache] = useState<Record<string, FileRecords>>({})
  const [graphCache, setGraphCache] = useState<Record<string, GraphData>>({})
  const [showHelp, setShowHelp] = useState(false)
  const [loadingFile, setLoadingFile] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  const router = useRouter()
  const [activeType, setActiveType] = useState<string>('')

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

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault()
      }
      if (e.altKey && e.key === 'ArrowLeft') router.back()
      if (e.altKey && e.key === 'ArrowRight') router.forward()
      if (e.key === '?') setShowHelp(v => !v)
      if (e.key === 'Escape') setShowHelp(false)
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [router])

  const openProject = useCallback(async () => {
    if (!api.isTauri) {
      setProject(MOCK_PROJECT)
      setFileDataCache(MOCK_FILE_RECORDS)
      return
    }
    const yamlPath = await api.pickProjectYaml()
    if (!yamlPath) return
    setErrorMsg(null)
    setFileDataCache({})
    setGraphCache({})
    try {
      const snapshot = await api.loadProject(yamlPath)
      setProject(snapshot)
      // Auto-open first source file
      const firstFile = collectSourceFiles(snapshot)[0]
      if (firstFile) router.push({ view: 'table', file: firstFile })
    } catch (err) {
      setErrorMsg(`打开项目失败: ${String(err)}`)
    }
  }, [router])

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
      .catch(err => setErrorMsg(`读取文件失败: ${String(err)}`))
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
      .catch(err => setErrorMsg(`读取图谱失败: ${String(err)}`))
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

  const currentRoute = router.current
  const activeFile = currentRoute?.file ?? null
  const activeFileData = activeFile ? fileDataCache[activeFile] : null
  const activeGraph = activeFile ? graphCache[activeFile] : null

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
        <span className="topbar-divider" />
        <button
          className="btn btn-icon"
          onClick={router.back}
          disabled={!router.canBack}
          title="后退 (Alt+←)"
        >
          <Icon name="arrow-left" size={14} />
        </button>
        <button
          className="btn btn-icon"
          onClick={router.forward}
          disabled={!router.canForward}
          title="前进 (Alt+→)"
        >
          <Icon name="arrow-right" size={14} />
        </button>
        {project && (
          <span className="project-root" title={project.project_root}>
            {project.project_root}
          </span>
        )}
        <span className="topbar-spacer" />
        <button className="btn btn-icon" onClick={() => setShowHelp(v => !v)} title="帮助 (?)">
          <Icon name="help" size={14} />
        </button>
      </div>

      {errorMsg && (
        <div className="error-banner">
          <Icon name="error" size={13} />
          {errorMsg}
          <button className="btn btn-icon" onClick={() => setErrorMsg(null)}>
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
                <Icon name="file" size={12} className="breadcrumb-icon" />
                {activeFile?.split('/').map((part, i, arr) => (
                  <span key={i} className="breadcrumb-part">
                    {i > 0 && <span className="breadcrumb-sep">/</span>}
                    <span className={i === arr.length - 1 ? 'breadcrumb-leaf' : ''}>{part}</span>
                  </span>
                ))}
              </div>

              {/* Type tabs row */}
              {activeFileData.type_names.length > 0 && (
                <div className="view-tabs view-tabs-types">
                  <div className="type-tabs-inline">
                    {activeFileData.type_names.map(t => (
                      <button
                        key={t}
                        className={`tab-btn${activeType === t ? ' active' : ''}`}
                        onClick={() => setActiveType(t)}
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
              <div className="view-tabs view-tabs-views">
                {(['table', 'record', 'graph'] as const).map(v => (
                  <button
                    key={v}
                    className={`tab-btn tab-view${currentRoute.view === v ? ' active' : ''}`}
                    onClick={() => switchView(v)}
                  >
                    <Icon name={v === 'table' ? 'table' : v === 'record' ? 'record' : 'graph'} size={13} />
                    {v === 'table' ? '表格' : v === 'record' ? '记录' : '图谱'}
                  </button>
                ))}
              </div>

              <div className="view-container">
                {currentRoute.view === 'table' && (
                  <TableView
                    data={activeFileData}
                    activeType={activeType}
                    onOpenRecord={key => openRecord(currentRoute.file, key)}
                  />
                )}
                {currentRoute.view === 'record' && (
                  <RecordView
                    data={activeFileData}
                    recordKey={currentRoute.recordKey}
                    typeFilter={activeType}
                    onOpenRecord={key => openRecord(currentRoute.file, key)}
                  />
                )}
                {currentRoute.view === 'graph' && (
                  activeGraph ? (
                    <GraphView
                      graphData={activeGraph}
                      activeType={activeType}
                      onOpenRecord={(file, key) => openRecord(file, key)}
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
                <button className="btn btn-primary" onClick={openProject}>
                  <Icon name="open" size={13} />
                  打开项目
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {project && (
        <DiagnosticsPanel
          diagnostics={project.diagnostics}
          onJumpToRecord={(file, key) => openRecord(file, key)}
        />
      )}

      {showHelp && (
        <div className="help-overlay" onClick={() => setShowHelp(false)}>
          <div className="help-box" onClick={e => e.stopPropagation()}>
            <h3>
              <Icon name="help" size={16} />
              键盘快捷键
            </h3>
            <table>
              <tbody>
                <tr><td>Alt+←</td><td>后退</td></tr>
                <tr><td>Alt+→</td><td>前进</td></tr>
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

function collectSourceFiles(snapshot: ProjectSnapshot): string[] {
  const out: string[] = []
  function walk(n: ProjectSnapshot['file_tree'][number]) {
    if (!n.is_dir && n.in_sources) out.push(n.path)
    for (const c of n.children) walk(c)
  }
  for (const n of snapshot.file_tree) walk(n)
  return out
}
