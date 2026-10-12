import { describe, expect, it } from 'vitest'
import { selectionEditIntentForKey } from './selectionKeyboard'

describe('selected value keyboard intent', () => {
  it('maps shared edit, clear, and bool toggle keys', () => {
    expect(selectionEditIntentForKey('x', false, 'string')).toEqual({ kind: 'replace', text: 'x' })
    expect(selectionEditIntentForKey('Enter', false, 'string')).toEqual({ kind: 'edit' })
    expect(selectionEditIntentForKey('Enter', false, 'bool')).toEqual({ kind: 'toggle-bool' })
    expect(selectionEditIntentForKey('Delete', false, 'object')).toEqual({ kind: 'clear' })
  })

  it('ignores navigation and modified input', () => {
    expect(selectionEditIntentForKey('ArrowDown', false, 'string')).toBeNull()
    expect(selectionEditIntentForKey('v', true, 'string')).toBeNull()
  })
})
