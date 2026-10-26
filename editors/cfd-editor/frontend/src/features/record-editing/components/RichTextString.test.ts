import { describe, expect, it } from 'vitest'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { parseRichText, RichTextString } from './RichTextString'

describe('parseRichText', () => {
  it('recognizes HTML and Unity rich text tags', () => {
    expect(parseRichText('<strong>HTML</strong>')).not.toBeNull()
    expect(parseRichText('<color=#ff0000>Unity</color>')).not.toBeNull()
    expect(parseRichText('<sprite=icon>')).not.toBeNull()
  })

  it('leaves ordinary strings on the plain rendering path', () => {
    expect(parseRichText('3 < 5 and no tags')).toBeNull()
    expect(parseRichText('literal <unknown>tag</unknown>')).toBeNull()
  })

  it('renders supported HTML and Unity styles without emitting source tags', () => {
    const markup = renderToStaticMarkup(createElement(RichTextString, {
      text: '<strong>HTML</strong> <color=#ff0000>Unity</color>',
    }))
    expect(markup).toContain('<strong>')
    expect(markup).toContain('HTML')
    expect(markup).toContain('style="color:#ff0000"')
    expect(markup).not.toContain('&lt;color')
  })
})
