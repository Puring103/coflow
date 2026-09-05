import { describe, expect, it } from 'vitest'
import { ChangeSet } from '@codemirror/state'
import {
  changesStayWithinEditableRange,
  completionPrefixPattern,
  completionPrefixValidPattern,
  mergeSemanticTokens,
} from './CfdCodeEditor'

describe('CFD completion triggering', () => {
  it('treats an annotation marker and its partial name as completion prefixes', () => {
    expect('  @'.match(completionPrefixPattern)?.[0]).toBe('@')
    expect('  @lab'.match(completionPrefixPattern)?.[0]).toBe('@lab')
    expect(completionPrefixValidPattern.test('@label')).toBe(true)
    expect('  {&standard.vis'.match(completionPrefixPattern)?.[0]).toBe('&standard.vis')
    expect(completionPrefixValidPattern.test('&Item::sword.name')).toBe(true)
  })
})

describe('CFD semantic highlighting', () => {
  it('retains recovered tokens missing from an invalid intermediate response', () => {
    expect(mergeSemanticTokens([
      { from: 0, to: 7, type: 'type' },
      { from: 12, to: 16, type: 'property' },
    ], [
      { from: 0, to: 7, type: 'recordKey' },
    ])).toEqual([
      { from: 0, to: 7, type: 'recordKey' },
      { from: 12, to: 16, type: 'property' },
    ])
  })
})

describe('CFD editable ranges', () => {
  const range = { from: 10, to: 20 }

  it('accepts changes wholly inside the function body', () => {
    expect(changesStayWithinEditableRange(ChangeSet.of({ from: 12, to: 15, insert: 'body' }, 30), range)).toBe(true)
  })

  it('rejects signature and closing-delimiter changes', () => {
    expect(changesStayWithinEditableRange(ChangeSet.of({ from: 2, to: 3, insert: 'x' }, 30), range)).toBe(false)
    expect(changesStayWithinEditableRange(ChangeSet.of({ from: 19, to: 21, insert: '' }, 30), range)).toBe(false)
  })
})
