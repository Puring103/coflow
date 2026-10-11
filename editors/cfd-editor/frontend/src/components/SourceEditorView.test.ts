import { describe, it, expect } from 'vitest'
import { normalizeSource } from '../state/sourceAutosave'

describe('source editor line endings', () => {
  it('normalizes display text without losing blank lines', () => {
    expect(normalizeSource('first\n\r\nsecond\rthird')).toBe('first\n\nsecond\nthird')
  })
})
