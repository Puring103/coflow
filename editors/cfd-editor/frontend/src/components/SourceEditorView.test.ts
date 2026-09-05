import { describe, expect, it } from 'vitest'
import { EditorState } from '@codemirror/state'
import { languageTextEditChanges, normalizeSourceText } from './SourceEditorView'

describe('source editor text normalization', () => {
  it('normalizes Windows and mixed line endings before dirty comparison', () => {
    expect(normalizeSourceText('first\n\r\nsecond\rthird')).toBe('first\n\nsecond\nthird')
  })
})

describe('source editor formatting edits', () => {
  it('converts independent LSP edits without replacing the whole document', () => {
    const source = 'type Item {\nname:string;\nvalue:int;\n}\n'
    expect(languageTextEditChanges(source, [
      {
        range: { start: { line: 1, character: 4 }, end: { line: 1, character: 4 } },
        new_text: ' ',
      },
      {
        range: { start: { line: 2, character: 0 }, end: { line: 2, character: 0 } },
        new_text: '  ',
      },
    ])).toEqual([
      { from: 16, to: 16, insert: ' ' },
      { from: 25, to: 25, insert: '  ' },
    ])
  })

  it('lets CodeMirror map a cursor through formatting changes', () => {
    const source = 'type Item {\nname:string;\nvalue:int;\n}\n'
    const cursor = source.indexOf('int')
    const changes = languageTextEditChanges(source, [
      {
        range: { start: { line: 1, character: 0 }, end: { line: 1, character: 0 } },
        new_text: '  ',
      },
      {
        range: { start: { line: 2, character: 5 }, end: { line: 2, character: 5 } },
        new_text: ' ',
      },
    ])
    const state = EditorState.create({ doc: source, selection: { anchor: cursor } })
    const next = state.update({ changes }).state

    expect(next.selection.main.head).toBe(cursor + 3)
    expect(next.sliceDoc(next.selection.main.head, next.selection.main.head + 3)).toBe('int')
  })
})
