import { describe, expect, it } from 'vitest'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { fieldPathField } from '../wire'
import { recordSelection, valueSelection } from './editorSelection'
import { editIntentForKey, moveTableSelection } from './tableCellNavigation'

const rows: RecordCoordinate[] = [
  { actual_type: 'Item', key: 'one' },
  { actual_type: 'Item', key: 'two' },
]
const columns = ['name', 'price']

describe('table cell navigation', () => {
  it('moves through record headers and value cells in visible order', () => {
    const record = recordSelection('data/items.cfd', rows[0])
    const name = moveTableSelection(record, 'ArrowRight', rows, columns)
    const price = moveTableSelection(name, 'ArrowRight', rows, columns)

    expect(name).toEqual(valueSelection('data/items.cfd', rows[0], [fieldPathField('name')]))
    expect(price).toEqual(valueSelection('data/items.cfd', rows[0], [fieldPathField('price')]))
    expect(moveTableSelection(name, 'ArrowLeft', rows, columns)).toEqual(record)
  })

  it('keeps the selected column while moving between visible rows', () => {
    const selection = valueSelection('data/items.cfd', rows[0], [fieldPathField('price')])

    expect(moveTableSelection(selection, 'ArrowDown', rows, columns)).toEqual(
      valueSelection('data/items.cfd', rows[1], [fieldPathField('price')]),
    )
    expect(moveTableSelection(selection, 'ArrowUp', rows, columns)).toBe(selection)
  })

  it('starts replacement for printable input and existing-value edit for Enter or F2', () => {
    expect(editIntentForKey('x', false)).toEqual({ kind: 'replace', text: 'x' })
    expect(editIntentForKey('Enter', false)).toEqual({ kind: 'edit' })
    expect(editIntentForKey('F2', false)).toEqual({ kind: 'edit' })
    expect(editIntentForKey('v', true)).toBeNull()
    expect(editIntentForKey('ArrowRight', false)).toBeNull()
  })
})
