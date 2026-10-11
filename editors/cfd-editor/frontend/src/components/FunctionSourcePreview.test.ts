import { describe, expect, it } from 'vitest'
import { functionBody, highlightFunctionBody } from './FunctionSourcePreview'

describe('function source preview', () => {
  it('extracts a nested body without treating braces in strings as structure', () => {
    const source = 'fn(value: int) -> string {\nif value > 0 { return "}" }\nreturn "{"\n}'
    expect(functionBody(source)).toBe('if value > 0 { return "}" }\nreturn "{"')
  })

  it('classifies body tokens used by the compact table highlighter', () => {
    expect(highlightFunctionBody('fn(value: int) -> int { return value + 12 }')).toEqual(expect.arrayContaining([
      { text: 'return', type: 'keyword' },
      { text: 'value', type: 'parameter' },
      { text: '+', type: 'operator' },
      { text: '12', type: 'number' },
    ]))
  })
})
