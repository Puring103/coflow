import { autocompletion, closeBrackets, closeBracketsKeymap, closeCompletion, completionKeymap, startCompletion, type Completion, type CompletionContext } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, indentOnInput, indentUnit } from '@codemirror/language'
import { lintGutter, setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { Annotation, ChangeSet, Compartment, EditorState, StateEffect, StateField, Transaction, type ChangeSpec, type Range } from '@codemirror/state'
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
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view'
import { useEffect, useRef } from 'react'

export interface CodeSemanticToken {
  from: number
  to: number
  type: string
}

export interface CodeLineDecoration {
  line: number
  className: string
}

interface SemanticTokenUpdate {
  tokens: readonly CodeSemanticToken[]
  replace: boolean
}

// 用户可见范围内的即时词法着色：LSP 语义 token 到达前先给出基础高亮，
// 到达后由 `cm-lsp-token-*` 覆盖细化。只依赖词法，不涉及 schema 或项目状态。
const BASE_HIGHLIGHT_KEYWORDS = new Set([
  'type', 'enum', 'const', 'abstract', 'sealed', 'check', 'when', 'all', 'any', 'none',
  'in', 'is', 'true', 'false', 'fn', 'var', 'return', 'if', 'else', 'match', 'for',
  'while', 'break', 'continue',
])
const BASE_HIGHLIGHT_TYPES = new Set(['int', 'float', 'bool', 'string', 'Option', 'Result', 'Some', 'Ok', 'Err'])
const BASE_HIGHLIGHT_PATTERN =
  /@[A-Za-z_][A-Za-z0-9_]*|"(?:\\.|[^"\\])*"|#[^\n]*|\b\d[\w.]*\b|[A-Za-z_][A-Za-z0-9_]*|[+\-*/%=<>!&|^~?:.]+/g

function baseHighlightClass(match: string, line: string, index: number): string | null {
  if (match.startsWith('@')) return 'decorator'
  if (match.startsWith('"')) return 'string'
  if (match.startsWith('#')) return 'comment'
  if (/^\d/.test(match)) return 'number'
  if (/^[A-Za-z_]/.test(match)) {
    if (BASE_HIGHLIGHT_KEYWORDS.has(match)) return 'keyword'
    if (BASE_HIGHLIGHT_TYPES.has(match) || /^[A-Z]/.test(match)) return 'type'
    let next = index + match.length
    while (next < line.length && (line[next] === ' ' || line[next] === '\t')) next += 1
    return line[next] === '(' ? 'function' : null
  }
  return 'operator'
}

function buildBaseHighlight(view: EditorView): DecorationSet {
  const ranges: Range<Decoration>[] = []
  for (const { from, to } of view.visibleRanges) {
    const firstLine = view.state.doc.lineAt(from).number
    const lastLine = view.state.doc.lineAt(to).number
    for (let lineNumber = firstLine; lineNumber <= lastLine; lineNumber += 1) {
      const line = view.state.doc.line(lineNumber)
      const text = line.text
      BASE_HIGHLIGHT_PATTERN.lastIndex = 0
      for (let match = BASE_HIGHLIGHT_PATTERN.exec(text); match; match = BASE_HIGHLIGHT_PATTERN.exec(text)) {
        const tokenClass = baseHighlightClass(match[0], text, match.index)
        if (tokenClass) {
          const start = line.from + match.index
          ranges.push(Decoration.mark({
            class: `cm-lsp-token cm-lsp-token-${tokenClass}`,
          }).range(start, start + match[0].length))
        }
      }
    }
  }
  return Decoration.set(ranges, true)
}

const baseHighlightPlugin = ViewPlugin.fromClass(class {
  decorations: DecorationSet
  constructor(view: EditorView) {
    this.decorations = buildBaseHighlight(view)
  }
  update(update: ViewUpdate) {
    if (update.docChanged || update.viewportChanged) {
      this.decorations = buildBaseHighlight(update.view)
    }
  }
}, { decorations: plugin => plugin.decorations })

const setSemanticTokens = StateEffect.define<SemanticTokenUpdate>()
const setLineDecorations = StateEffect.define<readonly CodeLineDecoration[]>()

const lineDecorationField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    for (const effect of transaction.effects) {
      if (!effect.is(setLineDecorations)) continue
      const ranges = effect.value.flatMap(item => {
        if (item.line < 1 || item.line > transaction.state.doc.lines) return []
        return [Decoration.line({ class: item.className }).range(
          transaction.state.doc.line(item.line).from,
        )]
      })
      return Decoration.set(ranges, true)
    }
    return value.map(transaction.changes)
  },
  provide: field => EditorView.decorations.from(field),
})

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

export const completionPrefixPattern = /[@&]?[\p{L}\p{N}_.:]*$/u
export const completionPrefixValidPattern = /^[@&]?[\p{L}\p{N}_.:]*$/u

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
  focusRange?: { from: number; to: number; tick: number } | null
  className?: string
  lineDecorations?: readonly CodeLineDecoration[]
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
  focusRange = null,
  className,
  lineDecorations = [],
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  const onSaveRef = useRef(onSave)
  const onCompleteRef = useRef(onComplete)
  const completionCompartment = useRef(new Compartment())
  const readOnlyCompartment = useRef(new Compartment())
  const composingRef = useRef(false)
  const compositionGenerationRef = useRef(0)
  onChangeRef.current = onChange
  onSaveRef.current = onSave
  onCompleteRef.current = onComplete

  const completionSource = (context: CompletionContext) => {
    if (composingRef.current) return null
    const complete = onCompleteRef.current
    if (!complete) return null
    const word = context.matchBefore(completionPrefixPattern)
    if (!word || (!context.explicit && word.from === word.to)) return null
    const compositionGeneration = compositionGenerationRef.current
    const line = context.state.doc.lineAt(context.pos)
    return complete(context.state.doc.toString(), {
      line: line.number - 1,
      character: context.pos - line.from,
    }).then(options => composingRef.current || compositionGeneration !== compositionGenerationRef.current ? null : ({
      from: word.from,
      options: [...options],
      validFor: completionPrefixValidPattern,
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
          baseHighlightPlugin,
          semanticTokenField,
          lineDecorationField,
          editableRangeField,
          EditorState.transactionFilter.of(transaction => {
            const range = transaction.startState.field(editableRangeField)
            if (!range || !transaction.docChanged || transaction.annotation(externalDocumentUpdate)) return transaction
            return changesStayWithinEditableRange(transaction.changes, range) ? transaction : []
          }),
          completionCompartment.current.of(
            autocompletion({ override: [completionSource] }),
          ),
          EditorView.domEventHandlers({
            // 中文输入法组合文本尚未确认时不能触发或接受代码补全。
            compositionstart: (_event, currentView) => {
              composingRef.current = true
              compositionGenerationRef.current += 1
              closeCompletion(currentView)
              return false
            },
            compositionend: (_event, currentView) => {
              composingRef.current = false
              requestAnimationFrame(() => {
                if (!composingRef.current && currentView.hasFocus) startCompletion(currentView)
              })
              return false
            },
          }),
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
            '.cm-tooltip-autocomplete ul li[aria-selected]': { backgroundColor: 'var(--bg-4)', color: 'var(--text)' },
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
    if (!view || !focusRange) return
    const from = Math.min(view.state.doc.length, Math.max(0, focusRange.from))
    const to = Math.min(view.state.doc.length, Math.max(from, focusRange.to))
    view.dispatch({
      selection: { anchor: from, head: to },
      effects: EditorView.scrollIntoView(from, { y: 'center' }),
    })
    view.focus()
  }, [focusRange])

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

  useEffect(() => {
    viewRef.current?.dispatch({ effects: setLineDecorations.of(lineDecorations) })
  }, [lineDecorations])

  return <div ref={hostRef} className={`cfd-code-editor${className ? ` ${className}` : ''}`} />
}
