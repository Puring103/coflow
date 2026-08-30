import type { Completion } from '@codemirror/autocomplete'
import type { Diagnostic } from '@codemirror/lint'
import { Text } from '@codemirror/state'
import type {
  LanguageCompletion,
  LanguageDiagnostic,
  LanguageDocumentState,
  LanguagePosition,
} from '../api'
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

function positionOffset(doc: Text, position: LanguagePosition): number {
  const line = doc.line(Math.min(doc.lines, position.line + 1))
  return Math.min(line.to, line.from + position.character)
}

export function completionItem(item: LanguageCompletion): Completion {
  const type = item.kind === 7 ? 'class' : item.kind === 5 ? 'property' : item.kind === 3 ? 'function' : 'text'
  return { label: item.label, detail: item.detail, type, apply: item.insert_text }
}
