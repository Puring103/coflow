import { expect, it } from 'vitest'
import { previewWindow, previewRowCount } from './tableExpansion'
import type { FieldValue } from '../../../wire'

it('stops before omitted children and leaves collapsed subtrees unread', () => {
  const unread: FieldValue = { kind: 'array', value: [] }
  Object.defineProperty(unread, 'value', { get() { throw new Error('hidden subtree visited') } })
  const items: FieldValue[] = [unread, unread, unread]
  Object.defineProperty(items, 3, { get() { throw new Error('omitted tail visited') } })
  const value: FieldValue = { kind: 'array', value: items }
  const preview = previewWindow(value, null, new Set(), 'items', 3)
  expect(preview.rows).toBe(3)
  expect(preview.windows.get('items')).toBe(3)
  expect(previewWindow(value, null, new Set(['items[0]']), 'items', 1).rows).toBe(1)
})

it('shares one height budget across expanded descendants and retains full height on demand', () => {
  const leaf: FieldValue = { kind: 'int', value: 1n }
  const value: FieldValue = { kind: 'array', value: [
    { kind: 'array', value: Array(100).fill(leaf) }, leaf,
  ] }
  const paths = new Set(['items[0]'])
  const preview = previewWindow(value, null, paths, 'items', 5)
  expect(preview.rows).toBe(5)
  expect(preview.windows.get('items')).toBe(1)
  expect(preview.windows.get('items[0]')).toBe(4)
  expect(previewRowCount(value, null, paths, 'items')).toBe(102)
})
