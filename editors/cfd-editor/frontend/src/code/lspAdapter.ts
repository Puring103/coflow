import type { Completion } from '@codemirror/autocomplete'
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

export function completionItem(item: LanguageCompletion): Completion {
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
  return { label: item.label, detail: item.detail, type, apply: item.insert_text }
}
