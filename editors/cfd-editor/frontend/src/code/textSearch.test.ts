import { describe, expect, it } from 'vitest'
import { textSearchMatches } from './textSearch'

describe('source text search', () => {
  it('treats regular-expression punctuation as literal text', () => {
    expect(textSearchMatches('a[0] + a[1] + A[0]', 'a[0]')).toEqual([
      { from: 0, to: 4 }, { from: 14, to: 18 },
    ])
  })
  it('preserves document offsets through Unicode and line breaks', () => {
    const source = '🌱中文\n中文'
    const matches = textSearchMatches(source, '中文')
    expect(matches).toEqual([{ from: 2, to: 4 }, { from: 5, to: 7 }])
    expect(textSearchMatches(source, '')).toEqual([])
    expect(textSearchMatches(source, '文\n中')).toEqual([{ from: 3, to: 6 }])
  })
})
