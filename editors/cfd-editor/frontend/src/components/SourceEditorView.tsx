import { useEffect, useMemo, useRef, useState } from 'react'
import * as api from '../api'
import type { ProjectSnapshot } from '../bindings/ProjectSnapshot'
import { errorDiagnostics, errorMessage, type DiagnosticItem } from '../wire'
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
  type ExternalDocumentUpdate,
} from './CfdCodeEditor'
import { Icon } from './Icon'

interface Draft {
  base: string
  text: string
}

const drafts = new Map<string, Draft>()

export function normalizeSourceText(text: string): string {
  return text.replace(/\r\n?/g, '\n')
}

export function languageTextEditChanges(source: string, edits: readonly api.LanguageTextEdit[]) {
  return edits.map(edit => ({
    ...rangeOffsets(source, edit.range),
    insert: normalizeSourceText(edit.new_text),
  })).sort((left, right) => left.from - right.from || left.to - right.to)
}

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
  const [validationDiagnostics, setValidationDiagnostics] = useState<DiagnosticItem[]>([])
  const [semanticTokens, setSemanticTokens] = useState<CodeSemanticToken[]>([])
  const [replaceSemanticTokens, setReplaceSemanticTokens] = useState(true)
  const [documentUpdate, setDocumentUpdate] = useState<ExternalDocumentUpdate | null>(null)
  const [error, setError] = useState<string | null>(null)
  const languageRequest = useRef(0)
  const languageVersion = useRef(0)
  const validationRequest = useRef(0)
  const loadedKey = useRef<string | null>(null)
  const immediateLanguageSource = useRef<string | null>(null)
  const dirty = source !== base
  const editorDiagnostics = useMemo(() => [
    ...codeMirrorDiagnostics(source, diagnostics),
    ...validationCodeMirrorDiagnostics(source, validationDiagnostics),
  ], [diagnostics, source, validationDiagnostics])

  useEffect(() => {
    let alive = true
    const changingDocument = loadedKey.current !== key
    if (changingDocument) {
      setLoading(true)
      setError(null)
      setValidationDiagnostics([])
    }
    const operation = api.isTauri
      ? api.readSourceText(sessionId, filePath)
      : Promise.resolve(drafts.get(key)?.base ?? '')
    operation.then(rawText => {
      if (!alive) return
      const text = normalizeSourceText(rawText)
      const draft = drafts.get(key)
      setBase(text)
      setSource(draft?.base === text ? draft.text : text)
      setDocumentUpdate(null)
      loadedKey.current = key
      if (draft?.base !== text) drafts.delete(key)
    }).catch(cause => {
      if (alive) setError(errorMessage(cause))
    }).finally(() => { if (alive && changingDocument) setLoading(false) })
    return () => { alive = false }
  }, [filePath, key, revision, sessionId])

  useEffect(() => {
    if (!dirty) drafts.delete(key)
    else drafts.set(key, { base, text: source })
  }, [base, dirty, key, source])

  useEffect(() => {
    if (immediateLanguageSource.current === source) {
      immediateLanguageSource.current = null
      return
    }
    const request = ++languageRequest.current
    if (!api.isTauri || loading) {
      setDiagnostics([])
      setSemanticTokens([])
      setReplaceSemanticTokens(true)
      return
    }
    const timer = window.setTimeout(() => {
      const version = ++languageVersion.current
      api.syncLanguageDocument(sessionId, filePath, source, version).then(next => {
        if (languageRequest.current === request) {
          setDiagnostics(next.diagnostics)
          setSemanticTokens(decodeSemanticTokens(source, next))
          setReplaceSemanticTokens(next.syntax_valid)
        }
      }).catch(cause => {
        if (languageRequest.current === request) {
          setDiagnostics([])
          setError(errorMessage(cause))
        }
      })
    }, 180)
    return () => window.clearTimeout(timer)
  }, [filePath, loading, sessionId, source])

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
  }, [filePath, loading, replaceSemanticTokens, sessionId, source])

  useEffect(() => () => {
    if (api.isTauri) void api.closeLanguageDocument(sessionId, filePath)
  }, [filePath, sessionId])

  async function save() {
    if (readOnly || saving || !api.isTauri) return
    setSaving(true)
    setError(null)
    try {
      const sourceBeforeFormatting = source
      const formatting = await api.formatLanguageDocument(
        sessionId,
        filePath,
        sourceBeforeFormatting,
        ++languageVersion.current,
      )
      const formatted = normalizeSourceText(formatting.text)
      if (formatted !== sourceBeforeFormatting) {
        setDocumentUpdate({
          before: sourceBeforeFormatting,
          after: formatted,
          changes: languageTextEditChanges(sourceBeforeFormatting, formatting.edits),
        })
        immediateLanguageSource.current = formatted
        const request = ++languageRequest.current
        const version = ++languageVersion.current
        setSource(formatted)
        try {
          const next = await api.syncLanguageDocument(sessionId, filePath, formatted, version)
          if (languageRequest.current === request) {
            setDiagnostics(next.diagnostics)
            setSemanticTokens(decodeSemanticTokens(formatted, next))
            setReplaceSemanticTokens(next.syntax_valid)
          }
        } catch (cause) {
          if (languageRequest.current === request) {
            setDiagnostics([])
            setError(errorMessage(cause))
          }
        }
      }
      if (formatted === base) {
        drafts.delete(key)
        return
      }
      const snapshot = await api.writeSourceText(sessionId, filePath, formatted)
      drafts.delete(key)
      setBase(formatted)
      setValidationDiagnostics([])
      await onSaved(snapshot)
    } catch (cause) {
      const details = errorDiagnostics(cause).filter(item => diagnosticBelongsToFile(item, filePath))
      setValidationDiagnostics(details)
      setError(details.length > 0 ? null : errorMessage(cause))
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
        {error && <span className="source-editor-error" role="alert" title={error}>{error}</span>}
      </header>
      <div className="source-editor-main">
        <CfdCodeEditor
          value={source}
          onChange={next => {
            setDocumentUpdate(null)
            setSource(next)
            setValidationDiagnostics([])
            setError(null)
          }}
          onSave={() => { void save() }}
          readOnly={readOnly || saving}
          documentUpdate={documentUpdate}
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
  const diagnosticPath = diagnostic.file_path?.replace(/\\/g, '/')
  const sourcePath = filePath.replace(/\\/g, '/')
  return diagnosticPath === sourcePath || diagnosticPath?.endsWith(`/${sourcePath}`) === true
}
