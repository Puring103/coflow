import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, type Completion, type CompletionContext } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, indentOnInput, indentUnit } from '@codemirror/language'
import { lintGutter, setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { Annotation, ChangeSet, Compartment, EditorState, StateEffect, StateField, Transaction, type ChangeSpec } from '@codemirror/state'
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  rectangularSelection,
  Decoration,
  type DecorationSet,
} from '@codemirror/view'
import { useEffect, useRef } from 'react'

export interface CodeSemanticToken {
  from: number
  to: number
  type: string
}

interface SemanticTokenUpdate {
  tokens: readonly CodeSemanticToken[]
  replace: boolean
}

const setSemanticTokens = StateEffect.define<SemanticTokenUpdate>()

export interface EditableRange {
  from: number
  to: number
}

export interface ExternalDocumentUpdate {
  before: string
  after: string
  changes: readonly ChangeSpec[]
}

const setEditableRange = StateEffect.define<EditableRange | null>()
const externalDocumentUpdate = Annotation.define<boolean>()

export function changesStayWithinEditableRange(changes: ChangeSet, range: EditableRange): boolean {
  let allowed = true
  changes.iterChangedRanges((from, to) => {
    if (from < range.from || to > range.to) allowed = false
  })
  return allowed
}

const editableRangeField = StateField.define<EditableRange | null>({
  create: () => null,
  update(value, transaction) {
    if (value && transaction.docChanged) {
      value = {
        from: transaction.changes.mapPos(value.from, -1),
        to: transaction.changes.mapPos(value.to, 1),
      }
    }
    for (const effect of transaction.effects) {
      if (effect.is(setEditableRange)) value = effect.value
    }
    return value
  },
  provide: field => EditorView.decorations.compute([field], state => {
    const range = state.field(field)
    if (!range) return Decoration.none
    const decorations = []
    if (range.from > 0) decorations.push(Decoration.mark({ class: 'cm-readonly-source' }).range(0, range.from))
    if (range.to < state.doc.length) decorations.push(Decoration.mark({ class: 'cm-readonly-source' }).range(range.to, state.doc.length))
    return Decoration.set(decorations)
  }),
})

export function mergeSemanticTokens(
  existing: readonly CodeSemanticToken[],
  incoming: readonly CodeSemanticToken[],
): CodeSemanticToken[] {
  return [
    ...existing.filter(token => !incoming.some(next => next.from < token.to && token.from < next.to)),
    ...incoming,
  ].sort((left, right) => left.from - right.from || left.to - right.to)
}

const semanticTokenField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setSemanticTokens)) {
        const incoming = effect.value.tokens.filter(
          token => token.from < token.to && token.from >= 0 && token.to <= transaction.state.doc.length,
        )
        const existing: CodeSemanticToken[] = []
        if (!effect.value.replace) {
          value.between(0, transaction.state.doc.length, (from, to, decoration) => {
            const type = decoration.spec.tokenType
            if (typeof type === 'string') existing.push({ from, to, type })
          })
        }
        const ranges = mergeSemanticTokens(existing, incoming).map(token => Decoration.mark({
          class: `cm-lsp-token cm-lsp-token-${token.type}`,
          tokenType: token.type,
        }).range(token.from, token.to))
        return Decoration.set(ranges)
      }
    }
    return value.map(transaction.changes)
  },
  provide: field => EditorView.decorations.from(field),
})

interface Props {
  value: string
  onChange: (value: string) => void
  onSave?: () => void
  readOnly?: boolean
  editableRange?: EditableRange | null
  autoFocus?: boolean
  semanticTokens?: readonly CodeSemanticToken[]
  replaceSemanticTokens?: boolean
  onComplete?: (source: string, position: { line: number; character: number }) => Promise<readonly Completion[]>
  diagnostics?: readonly Diagnostic[]
  documentUpdate?: ExternalDocumentUpdate | null
  className?: string
}

export function CfdCodeEditor({
  value,
  onChange,
  onSave,
  readOnly = false,
  editableRange = null,
  autoFocus = false,
  semanticTokens = [],
  replaceSemanticTokens = true,
  onComplete,
  diagnostics = [],
  documentUpdate = null,
  className,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  const onSaveRef = useRef(onSave)
  const onCompleteRef = useRef(onComplete)
  const completionCompartment = useRef(new Compartment())
  const readOnlyCompartment = useRef(new Compartment())
  onChangeRef.current = onChange
  onSaveRef.current = onSave
  onCompleteRef.current = onComplete

  const completionSource = (context: CompletionContext) => {
    const complete = onCompleteRef.current
    if (!complete) return null
    const word = context.matchBefore(/[\p{L}\p{N}_.]*/u)
    if (!word || (!context.explicit && word.from === word.to)) return null
    const line = context.state.doc.lineAt(context.pos)
    return complete(context.state.doc.toString(), {
      line: line.number - 1,
      character: context.pos - line.from,
    }).then(options => ({
      from: word.from,
      options: [...options],
      validFor: /^[\p{L}\p{N}_.]*$/u,
    }))
  }

  useEffect(() => {
    if (!hostRef.current) return
    const view = new EditorView({
      parent: hostRef.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          history(),
          drawSelection(),
          dropCursor(),
          EditorState.allowMultipleSelections.of(true),
          EditorState.tabSize.of(2),
          indentUnit.of('  '),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          rectangularSelection(),
          crosshairCursor(),
          highlightActiveLine(),
          semanticTokenField,
          editableRangeField,
          EditorState.transactionFilter.of(transaction => {
            const range = transaction.startState.field(editableRangeField)
            if (!range || !transaction.docChanged || transaction.annotation(externalDocumentUpdate)) return transaction
            return changesStayWithinEditableRange(transaction.changes, range) ? transaction : []
          }),
          completionCompartment.current.of(
            autocompletion({ override: [completionSource] }),
          ),
          lintGutter(),
          readOnlyCompartment.current.of(EditorState.readOnly.of(readOnly)),
          EditorView.lineWrapping,
          keymap.of([
            {
              key: 'Mod-s',
              preventDefault: true,
              run: () => { onSaveRef.current?.(); return true },
            },
            indentWithTab,
            ...closeBracketsKeymap,
            ...defaultKeymap,
            ...historyKeymap,
            ...completionKeymap,
          ]),
          EditorView.updateListener.of(update => {
            const userDocumentChange = update.transactions.some(transaction => (
              transaction.docChanged && !transaction.annotation(externalDocumentUpdate)
            ))
            if (userDocumentChange) onChangeRef.current(update.state.doc.toString())
          }),
          EditorView.theme({
            '&': { height: '100%', backgroundColor: 'var(--bg-1)', color: 'var(--text)' },
            '.cm-scroller': { fontFamily: "'JetBrains Mono', 'SF Mono', Consolas, monospace", lineHeight: '1.55' },
            '.cm-content': { caretColor: 'var(--code-caret)', padding: '12px 0' },
            '.cm-gutters': { backgroundColor: 'var(--bg-2)', color: 'var(--text-mute)', border: 'none' },
            '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--bg-3)' },
            '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--code-selection)' },
            '.cm-tooltip': { backgroundColor: 'var(--bg-2)', color: 'var(--text)', border: '1px solid var(--border)' },
            '.cm-tooltip-autocomplete ul li[aria-selected]': { backgroundColor: 'var(--accent)', color: 'white' },
            '.cm-diagnostic': { padding: '4px 8px' },
          }),
        ],
      }),
    })
    viewRef.current = view
    if (autoFocus) requestAnimationFrame(() => view.focus())
    return () => {
      view.destroy()
      viewRef.current = null
    }
    // The editor owns its document after construction. Prop changes are synced below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const view = viewRef.current
    if (!view || !documentUpdate || value !== documentUpdate.after) return
    if (view.state.doc.toString() !== documentUpdate.before) return
    view.dispatch({
      changes: documentUpdate.changes,
      annotations: [externalDocumentUpdate.of(true), Transaction.addToHistory.of(true)],
    })
  }, [documentUpdate, value])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    const current = view.state.doc.toString()
    if (current !== value) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
        annotations: [externalDocumentUpdate.of(true), Transaction.addToHistory.of(false)],
      })
    }
  }, [value])

  useEffect(() => {
    viewRef.current?.dispatch({ effects: setEditableRange.of(editableRange) })
  }, [editableRange])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    view.dispatch({ effects: readOnlyCompartment.current.reconfigure(EditorState.readOnly.of(readOnly)) })
  }, [readOnly])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    view.dispatch(setDiagnostics(view.state, [...diagnostics]))
  }, [diagnostics])

  useEffect(() => {
    viewRef.current?.dispatch({ effects: setSemanticTokens.of({ tokens: semanticTokens, replace: replaceSemanticTokens }) })
  }, [replaceSemanticTokens, semanticTokens])

  return <div ref={hostRef} className={`cfd-code-editor${className ? ` ${className}` : ''}`} />
}
