import { describe, expect, it } from 'vitest'
import { codeMirrorDiagnostics, decodeSemanticTokens } from './lspAdapter'

describe('LSP CodeMirror adapter', () => {
  it('decodes delta semantic tokens using the server legend', () => {
    expect(decodeSemanticTokens('alpha\nbeta', {
      diagnostics: [],
      semantic_token_data: [0, 0, 5, 7, 0, 1, 0, 4, 5, 0],
      semantic_token_types: ['namespace', 'type', 'enum', 'enumMember', 'property', 'variable', 'function', 'keyword'],
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
})
