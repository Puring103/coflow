import { describe, expect, it } from 'vitest'
import { codeMirrorDiagnostics, completionItem, decodeSemanticTokens, validationCodeMirrorDiagnostics } from './lspAdapter'

describe('LSP CodeMirror adapter', () => {
  it('decodes delta semantic tokens using the server legend', () => {
    expect(decodeSemanticTokens('alpha\nbeta', {
      diagnostics: [],
      semantic_token_data: [0, 0, 5, 7, 0, 1, 0, 4, 5, 0],
      semantic_token_types: ['namespace', 'type', 'enum', 'enumMember', 'property', 'variable', 'function', 'keyword'],
      syntax_valid: true,
    })).toEqual([
      { from: 0, to: 5, type: 'keyword' },
      { from: 6, to: 10, type: 'variable' },
    ])
  })

  it('maps LSP diagnostic ranges without interpreting CFD syntax', () => {
    expect(codeMirrorDiagnostics('first\nsecond', [{
      range: { start: { line: 1, character: 1 }, end: { line: 1, character: 4 } },
      severity: 1,
      message: 'server error',
    }])).toEqual([{ from: 7, to: 10, severity: 'error', message: 'server error' }])
  })

  it('maps runtime validation ranges to inline CodeMirror diagnostics', () => {
    expect(validationCodeMirrorDiagnostics('first\nprice: "wrong"', [{
      severity: 'error',
      code: 'DATA-TYPE',
      stage: 'DATA',
      message: 'expected int',
      file_path: 'data.cfd',
      actual_type: 'Product',
      record_key: 'draft',
      field_path: 'price',
      range: { start: { line: 1, character: 7 }, end: { line: 1, character: 14 } },
      contexts: [],
    }])).toEqual([{
      from: 13,
      to: 20,
      severity: 'error',
      message: 'DATA-TYPE: draft.price: expected int',
    }])
  })

  it('keeps LSP value completion kinds for editor icons', () => {
    expect(completionItem({ label: 'Rare', kind: 20 })).toMatchObject({ type: 'enum' })
    expect(completionItem({ label: 'true', kind: 14 })).toMatchObject({ type: 'keyword' })
    expect(completionItem({ label: 'fn', kind: 3 })).toMatchObject({ type: 'function' })
  })
})
