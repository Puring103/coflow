import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ViewEditorDialog } from './ViewEditorDialog'

function render(availableRelations: string[]) {
  return renderToStaticMarkup(createElement(ViewEditorDialog, {
    initial: null,
    availableFields: ['name', 'owner'],
    availableRelations,
    groups: [],
    onSubmit: () => {},
    onClose: () => {},
  }))
}

describe('ViewEditorDialog graph availability', () => {
  it('hides the graph view option when the type has no relations', () => {
    const html = render([])
    expect(html).toContain('表格视图')
    expect(html).not.toContain('图视图')
  })

  it('offers the graph view option when relations exist', () => {
    const html = render(['owner'])
    expect(html).toContain('图视图')
  })
})
