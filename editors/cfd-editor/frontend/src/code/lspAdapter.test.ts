import { describe, expect, it, vi } from 'vitest'
import { codeMirrorDiagnostics, completionItem, decodeSemanticTokens, validationCodeMirrorDiagnostics } from './lspAdapter'

describe('LSP CodeMirror adapter', () => {
  it('decodes delta semantic tokens using the server legend', () => {
    expect(decodeSemanticTokens('alpha\nbeta', {
      diagnostics: [],
      semantic_token_data: [0, 0, 5, 7, 0, 1, 0, 4, 5, 0],
      semantic_token_types: ['recordKey', 'type', 'enum', 'enumMember', 'property', 'variable', 'function', 'keyword'],
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
      id: 'type-error',
      severity: 'error',
      code: 'DATA-TYPE',
      stage: 'DATA',
      message: 'expected int',
      target: { kind: 'source', file_path: 'data.cfd', range: { start: { line: 1, character: 7 }, end: { line: 1, character: 14 } } },
      contexts: [],
    }])).toEqual([{
      from: 13,
      to: 20,
      severity: 'error',
      message: 'DATA-TYPE: expected int',
    }])
  })

  it('keeps LSP value completion kinds for editor icons', () => {
    expect(completionItem({ label: 'Rare', kind: 20 })).toMatchObject({ type: 'enum' })
    expect(completionItem({ label: 'true', kind: 14 })).toMatchObject({ type: 'keyword' })
    expect(completionItem({ label: 'fn', kind: 3 })).toMatchObject({ type: 'function' })
  })

  it('displays the exact text applied by a precise non-snippet edit', () => {
    const item = completionItem({
      label: 'subtitle',
      filter_text: 'sub',
      text_edit: {
        range: { start: { line: 1, character: 14 }, end: { line: 1, character: 50 } },
        new_text: '&OptionExample::without_subtitle.subtitle',
      },
    }, 'with_subtitle\n  subtitle: "{&OptionExample::without_subtitle.sub}"')
    expect(item.label).toBe('sub')
    expect(item.displayLabel).toBe('&OptionExample::without_subtitle.subtitle')
    expect(item.apply).toBeTypeOf('function')
  })

  it('replaces text typed after an asynchronous precise completion request', () => {
    const source = 'with_subtitle\n  subtitle: "{&OptionExample::without_subtitle.s}"'
    const item = completionItem({
      label: '&OptionExample::without_subtitle.subtitle',
      text_edit: {
        range: { start: { line: 1, character: 14 }, end: { line: 1, character: 36 } },
        new_text: '&OptionExample::without_subtitle.subtitle',
      },
    }, source)
    const dispatch = vi.fn()
    if (typeof item.apply !== 'function') throw new Error('expected precise completion apply function')

    item.apply({ dispatch } as never, item, 28, 53)

    expect(dispatch).toHaveBeenCalledWith({
      changes: {
        from: 28,
        to: 53,
        insert: '&OptionExample::without_subtitle.subtitle',
      },
    })
  })

  it('preserves completion documentation and precise text edits', () => {
    const item = completionItem({
      label: 'enabled',
      documentation: 'Whether the feature is enabled.',
      text_edit: {
        range: { start: { line: 1, character: 2 }, end: { line: 1, character: 4 } },
        new_text: 'enabled: ${1:true}',
      },
      insert_text_format: 2,
    }, 'root\n  en')
    expect(item.info).toBe('Whether the feature is enabled.')
    expect(item.apply).toBeTypeOf('function')
  })
})
