import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, type Completion, type CompletionContext } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, indentOnInput, indentUnit } from '@codemirror/language'
import { lintGutter, setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { Compartment, EditorState, StateEffect, StateField } from '@codemirror/state'
import { indentationMarkers } from '@replit/codemirror-indentation-markers'
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

const setSemanticTokens = StateEffect.define<readonly CodeSemanticToken[]>()
const semanticTokenField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setSemanticTokens)) {
        const ranges = effect.value
          .filter(token => token.from < token.to && token.from >= 0 && token.to <= transaction.state.doc.length)
          .map(token => Decoration.mark({ class: `cm-lsp-token cm-lsp-token-${token.type}` }).range(token.from, token.to))
          .sort((left, right) => left.from - right.from || left.to - right.to)
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
  autoFocus?: boolean
  semanticTokens?: readonly CodeSemanticToken[]
  onComplete?: (position: { line: number; character: number }) => Promise<readonly Completion[]>
  diagnostics?: readonly Diagnostic[]
  className?: string
}

export function CfdCodeEditor({
  value,
  onChange,
  onSave,
  readOnly = false,
  autoFocus = false,
  semanticTokens = [],
  onComplete,
  diagnostics = [],
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
    return complete({ line: line.number - 1, character: context.pos - line.from }).then(options => ({
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
          EditorState.tabSize.of(4),
          indentUnit.of('    '),
          indentOnInput(),
          indentationMarkers({
            highlightActiveBlock: false,
            hideFirstIndent: true,
            markerType: 'fullScope',
            thickness: 1,
            colors: {
              light: 'var(--code-indent-guide)',
              dark: 'var(--code-indent-guide)',
            },
          }),
          bracketMatching(),
          closeBrackets(),
          rectangularSelection(),
          crosshairCursor(),
          highlightActiveLine(),
          semanticTokenField,
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
            if (update.docChanged) onChangeRef.current(update.state.doc.toString())
          }),
          EditorView.theme({
            '&': { height: '100%', backgroundColor: 'var(--bg-1)', color: 'var(--text)' },
            '.cm-scroller': { fontFamily: "'JetBrains Mono', 'SF Mono', Consolas, monospace", lineHeight: '1.55' },
            '.cm-content': { caretColor: 'var(--code-caret)', padding: '12px 0' },
            '.cm-gutters': { backgroundColor: 'var(--bg-2)', color: 'var(--text-mute)', border: 'none' },
            '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--bg-3)' },
            '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--bg-4)' },
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
    if (!view) return
    const current = view.state.doc.toString()
    if (current !== value) {
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } })
    }
  }, [value])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    view.dispatch({ effects: readOnlyCompartment.current.reconfigure(EditorState.readOnly.of(readOnly)) })
  }, [readOnly])

  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    view.dispatch(setDiagnostics(view.state, [...diagnostics]))
  }, [diagnostics, value])

  useEffect(() => {
    viewRef.current?.dispatch({ effects: setSemanticTokens.of(semanticTokens) })
  }, [semanticTokens])

  return <div ref={hostRef} className={`cfd-code-editor${className ? ` ${className}` : ''}`} />
}
