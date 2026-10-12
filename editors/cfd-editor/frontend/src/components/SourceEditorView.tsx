import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import * as api from '../api'
import type { ProjectBootstrap } from '../bindings/ProjectBootstrap'
import type { TextRange } from '../bindings/TextRange'
import { diagnosticFilePath, errorMessage, type DiagnosticItem } from '../wire'
import {
  codeMirrorDiagnostics,
  completionItem,
  decodeSemanticTokens,
  rangeOffsets,
  validationCodeMirrorDiagnostics,
} from '../code/lspAdapter'
import {
  CfdCodeEditor,
  type CodeSemanticToken,
} from './CfdCodeEditor'
import { SourceAutosave, registerSourceSave } from '../state/sourceAutosave'
import { Icon } from './Icon'

interface Props {
  sessionId: number
  revision: number
  filePath: string
  readOnly: boolean
  onSaved: (result: ProjectBootstrap) => Promise<void> | void
  focus?: { range: TextRange | null; tick: number } | null
}

export function SourceEditorView({ sessionId, revision, filePath, readOnly, onSaved, focus = null }: Props) {
  const onSavedRef = useRef(onSaved)
  onSavedRef.current = onSaved
  const controller = useMemo(() => new SourceAutosave({
    read: () => api.isTauri ? api.readSourceText(sessionId, filePath) : Promise.resolve(''),
    write: (text, expected) => api.writeSourceText(sessionId, filePath, text, expected),
    saved: (result: ProjectBootstrap) => onSavedRef.current(result),
    message: errorMessage,
  }), [sessionId, filePath])
  const state = useSyncExternalStore(controller.subscribe, controller.snapshot)
  const { text: source, saving, error } = state
  const loading = !state.loaded
  const dirty = controller.dirty
  const [languageError, setLanguageError] = useState<string | null>(null)
  const [diagnostics, setDiagnostics] = useState<api.LanguageDiagnostic[]>([])
  const [validationDiagnostics, setValidationDiagnostics] = useState<DiagnosticItem[]>([])
  const [semanticTokens, setSemanticTokens] = useState<CodeSemanticToken[]>([])
  const [replaceSemanticTokens, setReplaceSemanticTokens] = useState(true)
  const languageRequest = useRef(0)
  const languageVersion = useRef(0)
  const validationRequest = useRef(0)
  // 文档刚读取完成时立即请求一次语义着色，不等待编辑去抖；后续编辑仍去抖。
  const pendingInitialSync = useRef(false)
  const editorDiagnostics = useMemo(() => [
    ...codeMirrorDiagnostics(source, diagnostics),
    ...validationCodeMirrorDiagnostics(source, validationDiagnostics),
  ], [diagnostics, source, validationDiagnostics])
  const focusRange = useMemo(() => focus?.range
    ? { ...rangeOffsets(source, focus.range), tick: focus.tick }
    : null,
  [focus, source])

  useEffect(() => {
    let alive = true
    const baseline = controller.state.base
    const operation = api.isTauri ? api.readSourceText(sessionId, filePath) : Promise.resolve('')
    operation.then(text => {
      if (!alive || controller.state.base !== baseline) return
      controller.receiveDisk(text)
      pendingInitialSync.current = true
    }).catch(cause => { if (alive) controller.fail(cause) })
    return () => { alive = false }
  }, [controller, filePath, revision, sessionId])

  useEffect(() => registerSourceSave({
    pending: () => api.isTauri && (controller.dirty || controller.state.conflict !== null),
    flush: controller.flush,
  }), [controller])

  useEffect(() => {
    if (!api.isTauri || readOnly || loading || !dirty || saving || error || state.conflict !== null) return
    const timer = window.setTimeout(() => { void controller.flush() }, 500)
    return () => window.clearTimeout(timer)
  }, [controller, source, readOnly, loading, dirty, saving, error, state.conflict])

  useEffect(() => {
    const leave = (event: BeforeUnloadEvent) => {
      if (controller.dirty) { event.preventDefault(); event.returnValue = '' }
    }
    window.addEventListener('beforeunload', leave)
    return () => window.removeEventListener('beforeunload', leave)
  }, [controller])

  useEffect(() => {
    const request = ++languageRequest.current
    if (!api.isTauri || loading) {
      setDiagnostics([])
      setSemanticTokens([])
      setReplaceSemanticTokens(true)
      return
    }
    const immediate = pendingInitialSync.current
    pendingInitialSync.current = false
    const timer = window.setTimeout(() => {
      const version = ++languageVersion.current
      api.syncLanguageDocument(sessionId, filePath, source, version).then(next => {
        if (languageRequest.current === request) {
          setLanguageError(null)
          setDiagnostics(next.diagnostics)
          setSemanticTokens(decodeSemanticTokens(source, next))
          setReplaceSemanticTokens(next.syntax_valid)
        }
      }).catch(cause => {
        if (languageRequest.current === request) {
          setDiagnostics([])
          setLanguageError(errorMessage(cause))
        }
      })
    }, immediate ? 0 : 180)
    return () => window.clearTimeout(timer)
  }, [controller, filePath, loading, revision, sessionId, source])

  useEffect(() => {
    const request = ++validationRequest.current
    if (!api.isTauri || loading || !replaceSemanticTokens) {
      setValidationDiagnostics([])
      return
    }
    const timer = window.setTimeout(() => {
      api.validateSourceText(sessionId, filePath, source).then(next => {
        if (validationRequest.current === request) {
          setValidationDiagnostics(next.filter(item => diagnosticBelongsToFile(item, filePath)))
        }
      }).catch(() => {
        // LSP diagnostics remain available when project-level validation cannot run.
      })
    }, 450)
    return () => window.clearTimeout(timer)
  }, [filePath, loading, revision, replaceSemanticTokens, sessionId, source])

  useEffect(() => () => {
    if (api.isTauri) void api.closeLanguageDocument(sessionId, filePath)
  }, [filePath, sessionId])

  useEffect(() => {
    if (!api.isTauri) return
    const window = getCurrentWindow()
    const stop = window.onCloseRequested(event => {
      if (!controller.dirty) return
      event.preventDefault()
      void controller.flush().then(ok => { if (ok) void window.close() })
    })
    return () => { void stop.then(unlisten => unlisten()) }
  }, [controller])

  async function save() {
    if (readOnly || !api.isTauri) return
    await controller.flush()
  }

  if (loading) return <div className="empty-hint">{error ?? '加载源码中...'}</div>

  return (
    <section className="source-editor-view">
      <header className="source-editor-toolbar">
        <div className="source-editor-file">
          <Icon name="code" size={14} aria-hidden />
          <span>{filePath}</span>
          <span>{saving ? '保存中…' : dirty ? '等待保存' : '已保存'}</span>
          {dirty && <span className="source-editor-dirty" aria-label="有未保存更改" />}
        </div>
        {(error || languageError) && <span className="source-editor-error" role="alert">{error || languageError}</span>}
      </header>
      {state.conflict !== null && <div role="alert">
        磁盘文件已变化，当前输入已保留。
        <button onClick={() => controller.useDisk()}>使用磁盘内容</button>
        <button onClick={() => { void controller.keepLocal() }}>保留本地并覆盖</button>
      </div>}
      {error && state.conflict === null && <button onClick={() => { void save() }}>重试保存</button>}
      <div className="source-editor-main" onBlur={event => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null) && !readOnly && api.isTauri && !error) void controller.flush()
      }}>
        <CfdCodeEditor
          value={source}
          onChange={next => {
            controller.edit(next)
            setValidationDiagnostics([])
          }}
          onSave={() => { void save() }}
          readOnly={readOnly}
          focusRange={focusRange}
          semanticTokens={semanticTokens}
          replaceSemanticTokens={replaceSemanticTokens}
          diagnostics={editorDiagnostics}
          onComplete={async (currentSource, position) => {
            if (!api.isTauri) return []
            const items = await api.completeLanguageDocument(
              sessionId,
              filePath,
              currentSource,
              ++languageVersion.current,
              position,
            )
            return items.map(item => completionItem(item, currentSource))
          }}
          autoFocus
        />
      </div>
    </section>
  )
}

function diagnosticBelongsToFile(diagnostic: DiagnosticItem, filePath: string): boolean {
  const diagnosticPath = diagnosticFilePath(diagnostic)?.replace(/\\/g, '/')
  const sourcePath = filePath.replace(/\\/g, '/')
  return diagnosticPath === sourcePath || diagnosticPath?.endsWith(`/${sourcePath}`) === true
}
