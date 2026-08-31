import { snippet, type Completion } from '@codemirror/autocomplete'
import type { Diagnostic } from '@codemirror/lint'
import { Text } from '@codemirror/state'
import type {
  LanguageCompletion,
  LanguageDiagnostic,
  LanguageDocumentState,
  LanguagePosition,
  LanguageRange,
} from '../api'
import type { FlatDiagnostic } from '../bindings/FlatDiagnostic'
import type { CodeSemanticToken } from '../components/CfdCodeEditor'

export function decodeSemanticTokens(source: string, state: LanguageDocumentState): CodeSemanticToken[] {
  const doc = Text.of(source.split('\n'))
  const tokens: CodeSemanticToken[] = []
  let line = 0
  let character = 0
  for (let index = 0; index + 4 < state.semantic_token_data.length; index += 5) {
    const deltaLine = state.semantic_token_data[index]
    line += deltaLine
    character = deltaLine === 0 ? character + state.semantic_token_data[index + 1] : state.semantic_token_data[index + 1]
    if (line >= doc.lines) continue
    const documentLine = doc.line(line + 1)
    const from = Math.min(documentLine.to, documentLine.from + character)
    const to = Math.min(documentLine.to, from + state.semantic_token_data[index + 2])
    const type = state.semantic_token_types[state.semantic_token_data[index + 3]]
    if (type && from < to) tokens.push({ from, to, type })
  }
  return tokens
}

export function codeMirrorDiagnostics(source: string, diagnostics: readonly LanguageDiagnostic[]): Diagnostic[] {
  const doc = Text.of(source.split('\n'))
  return diagnostics.map(item => ({
    from: positionOffset(doc, item.range.start),
    to: positionOffset(doc, item.range.end),
    severity: item.severity === 1 ? 'error' : item.severity === 2 ? 'warning' : 'info',
    message: item.message,
  }))
}

export function validationCodeMirrorDiagnostics(
  source: string,
  diagnostics: readonly FlatDiagnostic[],
): Diagnostic[] {
  const doc = Text.of(source.split('\n'))
  return diagnostics.flatMap(item => item.range ? [{
    from: positionOffset(doc, item.range.start),
    to: positionOffset(doc, item.range.end),
    severity: item.severity === 'error' ? 'error' as const : item.severity === 'warning' ? 'warning' as const : 'info' as const,
    message: `${item.code}: ${flatDiagnosticMessage(item)}`,
  }] : [])
}

export function flatDiagnosticMessage(diagnostic: FlatDiagnostic): string {
  const location = [diagnostic.record_key, diagnostic.field_path].filter(Boolean).join('.')
  return location ? `${location}: ${diagnostic.message}` : diagnostic.message
}

function positionOffset(doc: Text, position: LanguagePosition): number {
  const line = doc.line(Math.min(doc.lines, position.line + 1))
  return Math.min(line.to, line.from + position.character)
}

export function rangeOffsets(source: string, range: LanguageRange): { from: number; to: number } {
  const doc = Text.of(source.split('\n'))
  return { from: positionOffset(doc, range.start), to: positionOffset(doc, range.end) }
}

export function completionItem(item: LanguageCompletion, source = ''): Completion {
  const type = item.kind === 7
    ? 'class'
    : item.kind === 5
      ? 'property'
      : item.kind === 3
        ? 'function'
        : item.kind === 20
          ? 'enum'
          : item.kind === 14
            ? 'keyword'
            : item.kind === 21
              ? 'constant'
              : 'text'
  const inserted = item.text_edit?.new_text ?? item.insert_text
  let apply: Completion['apply'] = inserted ?? (item.filter_text ? item.label : undefined)
  if (inserted && item.insert_text_format === 2) apply = snippet(inserted)
  if (inserted && item.text_edit) {
    const range = rangeOffsets(source, item.text_edit.range)
    const insert = item.insert_text_format === 2 ? snippet(inserted) : inserted
    apply = (view, completion, _from, _to) => {
      if (typeof insert === 'string') {
        view.dispatch({ changes: { from: range.from, to: range.to, insert } })
      } else {
        insert(view, completion, range.from, range.to)
      }
    }
  }
  return {
    label: item.filter_text ?? item.label,
    displayLabel: item.filter_text ? item.label : undefined,
    detail: item.detail,
    info: item.documentation,
    type,
    apply,
    boost: item.sort_text?.startsWith('0') ? 50 : undefined,
  }
}
