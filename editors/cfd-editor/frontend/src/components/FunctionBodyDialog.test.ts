import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { FunctionEditorButton } from './FunctionBodyDialog'

describe('FunctionEditorButton', () => {
  it('shows only the highlighted function body in the table control', () => {
    const html = renderToStaticMarkup(createElement(FunctionEditorButton, {
      value: {
        kind: 'function',
        value: { source: 'fn(left: int, right: int) -> int {\n  left + right\n}' },
      },
      onCommit: () => {},
    }))

    expect(html).toContain('<span class="cm-lsp-token-parameter">left</span>')
    expect(html).toContain('<span class="cm-lsp-token-operator">+</span>')
    expect(html).toContain('<span class="cm-lsp-token-parameter">right</span>')
    expect(html).not.toContain('fn(left')
  })
})
