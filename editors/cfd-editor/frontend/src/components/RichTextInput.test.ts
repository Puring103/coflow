import { describe, expect, it } from 'vitest'
import {
  applyRichTextCompletion,
  richTextCompletionContext,
  richTextSuggestions,
} from './RichTextInput'
import { parseRichText } from './RichTextString'

describe('rich-text input completion', () => {
  it('offers supported HTML and Unity tags for the current prefix', () => {
    expect(richTextSuggestions('text <co', 8).map(item => item.tag)).toEqual(['color', 'code'])
    expect(richTextSuggestions('text <', 6).some(item => item.tag === 'b')).toBe(true)
  })

  it('only offers snippets recognized by the rich-text renderer', () => {
    for (const completion of richTextSuggestions('<', 1)) {
      const inserted = applyRichTextCompletion('<', 1, completion).value
      expect(parseRichText(inserted), completion.tag).not.toBeNull()
    }
  })

  it('does not offer completions outside a tag prefix', () => {
    expect(richTextCompletionContext('text <b>done', 12)).toBeNull()
    expect(richTextSuggestions('ordinary text', 13)).toEqual([])
  })

  it('inserts paired tags and leaves the cursor inside them', () => {
    expect(applyRichTextCompletion('Use <b', 6, { tag: 'b', format: 'HTML / Unity' })).toEqual({
      value: 'Use <b></b>',
      cursor: 7,
    })
  })

  it('completes closing tags without adding another pair', () => {
    expect(applyRichTextCompletion('<b>text</b', 10, { tag: 'b', format: 'HTML / Unity' })).toEqual({
      value: '<b>text</b>',
      cursor: 11,
    })
  })
})
