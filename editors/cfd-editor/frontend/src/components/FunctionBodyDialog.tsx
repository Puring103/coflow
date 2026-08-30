import { useEffect, useRef, useState } from 'react'
import * as api from '../api'
import { codeMirrorDiagnostics, completionItem, decodeSemanticTokens } from '../code/lspAdapter'
import { useEditorLookups } from '../utils/editContext'
import type { FieldValue } from '../wire'
import { CfdCodeEditor } from './CfdCodeEditor'
import { Icon } from './Icon'

interface Props {
  value: FieldValue & { kind: 'function' }
  onCommit: (value: FieldValue) => void
  onClose: () => void
}

export function FunctionBodyDialog({ value, onCommit, onClose }: Props) {
  const { sessionId } = useEditorLookups()
  const [body, setBody] = useState('')
  const [document, setDocument] = useState<api.FunctionDocumentState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const request = useRef(0)

  useEffect(() => {
    const current = ++request.current
    void api.functionDocument(sessionId, value.value.source).then(next => {
      if (request.current !== current) return
      setDocument(next)
      setBody(next.body)
    }).catch(cause => {
      if (request.current === current) setError(cause instanceof Error ? cause.message : String(cause))
    })
  }, [sessionId, value.value.source])

  useEffect(() => {
    if (!document || body === document.body) return
    const current = ++request.current
    const timer = window.setTimeout(() => {
      void api.functionDocument(sessionId, value.value.source, body).then(next => {
        if (request.current === current) setDocument(next)
      }).catch(cause => {
        if (request.current === current) setError(cause instanceof Error ? cause.message : String(cause))
      })
    }, 120)
    return () => window.clearTimeout(timer)
  }, [body, document, sessionId, value.value.source])

  function save() {
    if (!document || document.body !== body || document.diagnostics.some(item => item.severity === 1)) return
    onCommit({ kind: 'function', value: { source: document.source } })
    onClose()
  }

  return (
    <div className="create-record-backdrop" role="presentation" onMouseDown={event => {
      if (event.target === event.currentTarget) onClose()
    }}>
      <section
        className="function-editor-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="编辑函数体"
        onMouseDown={event => event.stopPropagation()}
        onKeyDown={event => { if (event.key === 'Escape') onClose() }}
      >
        <header className="function-editor-header">
          <div className="function-editor-title">
            <Icon name="code" size={15} aria-hidden />
            <span>函数体</span>
          </div>
          <code className="function-signature">{document?.signature ?? 'fn'}</code>
          <button className="btn btn-icon" onClick={onClose} aria-label="关闭函数编辑器">
            <Icon name="close" size={14} />
          </button>
        </header>
        <div className="function-editor-body">
          {document && <CfdCodeEditor
              value={body}
              onChange={setBody}
              onSave={save}
              semanticTokens={decodeSemanticTokens(body, document)}
              diagnostics={codeMirrorDiagnostics(body, document.diagnostics)}
              onComplete={async () => document.completions.map(completionItem)}
              autoFocus
            />}
        </div>
        {error && <div className="function-editor-error" role="alert">{error}</div>}
        <footer className="function-editor-footer">
          <button className="btn" onClick={onClose}>取消</button>
          <button className="btn btn-primary" onClick={save} disabled={!document || document.body !== body || document.diagnostics.some(item => item.severity === 1)}>
            <Icon name="save" size={13} aria-hidden />
            保存
          </button>
        </footer>
      </section>
    </div>
  )
}

export function FunctionEditorButton({
  value,
  onCommit,
}: Pick<Props, 'value' | 'onCommit'>) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        type="button"
        className="function-editor-trigger"
        onClick={event => { event.stopPropagation(); setOpen(true) }}
        title="编辑函数体"
      >
        <Icon name="code" size={13} aria-hidden />
        <code>fn {'{ ... }'}</code>
      </button>
      {open && (
        <FunctionBodyDialog
          value={value}
          onCommit={onCommit}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  )
}
