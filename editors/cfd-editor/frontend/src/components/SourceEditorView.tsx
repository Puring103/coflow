import { useEffect, useRef, useState } from 'react'
import * as api from '../api'
import type { ProjectSnapshot } from '../bindings/ProjectSnapshot'
import { errorMessage } from '../wire'
import { codeMirrorDiagnostics, completionItem, decodeSemanticTokens } from '../code/lspAdapter'
import { CfdCodeEditor, type CodeSemanticToken } from './CfdCodeEditor'
import { Icon } from './Icon'

interface Draft {
  base: string
  text: string
}

const drafts = new Map<string, Draft>()

interface Props {
  sessionId: number
  revision: number
  filePath: string
  readOnly: boolean
  onSaved: (snapshot: ProjectSnapshot) => Promise<void> | void
}

export function SourceEditorView({ sessionId, revision, filePath, readOnly, onSaved }: Props) {
  const key = `${sessionId}:${filePath}`
  const [base, setBase] = useState('')
  const [source, setSource] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [diagnostics, setDiagnostics] = useState<api.LanguageDiagnostic[]>([])
  const [semanticTokens, setSemanticTokens] = useState<CodeSemanticToken[]>([])
  const [error, setError] = useState<string | null>(null)
  const languageRequest = useRef(0)
  const languageVersion = useRef(0)
  const dirty = source !== base

  useEffect(() => {
    let alive = true
    setLoading(true)
    setError(null)
    const operation = api.isTauri
      ? api.readSourceText(sessionId, filePath)
      : Promise.resolve(drafts.get(key)?.base ?? '')
    operation.then(text => {
      if (!alive) return
      const draft = drafts.get(key)
      setBase(text)
      setSource(draft?.base === text ? draft.text : text)
      if (draft?.base !== text) drafts.delete(key)
    }).catch(cause => {
      if (alive) setError(errorMessage(cause))
    }).finally(() => { if (alive) setLoading(false) })
    return () => { alive = false }
  }, [filePath, key, revision, sessionId])

  useEffect(() => {
    if (!dirty) drafts.delete(key)
    else drafts.set(key, { base, text: source })
  }, [base, dirty, key, source])

  useEffect(() => {
    const request = ++languageRequest.current
    if (!api.isTauri || loading) {
      setDiagnostics([])
      setSemanticTokens([])
      return
    }
    const version = ++languageVersion.current
    const timer = window.setTimeout(() => {
      api.syncLanguageDocument(sessionId, filePath, source, version).then(next => {
        if (languageRequest.current === request) {
          setDiagnostics(next.diagnostics)
          setSemanticTokens(decodeSemanticTokens(source, next))
        }
      }).catch(cause => {
        if (languageRequest.current === request) {
          setDiagnostics([])
          setSemanticTokens([])
          setError(errorMessage(cause))
        }
      })
    }, 180)
    return () => window.clearTimeout(timer)
  }, [filePath, loading, sessionId, source])

  useEffect(() => () => {
    if (api.isTauri) void api.closeLanguageDocument(sessionId, filePath)
  }, [filePath, sessionId])

  async function save() {
    if (!dirty || readOnly || saving || !api.isTauri) return
    setSaving(true)
    setError(null)
    try {
      const snapshot = await api.writeSourceText(sessionId, filePath, source)
      drafts.delete(key)
      setBase(source)
      await onSaved(snapshot)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <div className="empty-hint">加载源码中...</div>

  return (
    <section className="source-editor-view">
      <header className="source-editor-toolbar">
        <div className="source-editor-file">
          <Icon name="code" size={14} aria-hidden />
          <span>{filePath}</span>
          {dirty && <span className="source-editor-dirty" aria-label="有未保存更改" />}
        </div>
        <div className="source-editor-actions">
          {error && <span className="source-editor-error" role="alert">{error}</span>}
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => { void save() }}
            disabled={!dirty || readOnly || saving || !api.isTauri}
          >
            <Icon name="save" size={13} aria-hidden />
            {saving ? '保存中...' : '保存'}
          </button>
        </div>
      </header>
      <div className="source-editor-main">
        <CfdCodeEditor
          value={source}
          onChange={setSource}
          onSave={() => { void save() }}
          readOnly={readOnly}
          semanticTokens={semanticTokens}
          diagnostics={codeMirrorDiagnostics(source, diagnostics)}
          onComplete={async position => {
            if (!api.isTauri) return []
            const items = await api.completeLanguageDocument(
              sessionId,
              filePath,
              source,
              languageVersion.current,
              position,
            )
            return items.map(completionItem)
          }}
          autoFocus
        />
      </div>
      {diagnostics.length > 0 && (
        <div className="source-editor-diagnostics" role="status">
          {diagnostics.map((item, index) => (
            <div key={`${item.code ?? item.message}:${index}`} className={`source-editor-diagnostic ${severityName(item.severity)}`}>
              <Icon name={item.severity === 1 ? 'error' : item.severity === 2 ? 'warning' : 'info'} size={12} aria-hidden />
              {item.code && <code>{item.code}</code>}
              {item.message}
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

function severityName(severity: number): string {
  return severity === 1 ? 'error' : severity === 2 ? 'warning' : 'info'
}
