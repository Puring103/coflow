import { describe, expect, it } from 'vitest'
import { fromIpc } from './ipc'
import { decodeFileRecords } from './fileRecordsTransfer'
import type { FileRecordsTransfer } from '../bindings/FileRecordsTransfer'

describe('snapshot receive boundary', () => {
  it('preserves unchanged branches and never mutates received integer values', () => {
    const metadata = { label: 'Title', children: [] }
    const raw = { metadata, values: [{ kind: 'int', value: '9223372036854775807' },
      { kind: 'enum', value: { enum_name: 'Flags', value: '-9223372036854775808', variant: null } },
      { enum_int_value: '9007199254740993' }] }
    const normalized = fromIpc(raw)
    expect(normalized).not.toBe(raw)
    expect(normalized.metadata).toBe(metadata)
    expect(normalized.values[0].value).toBe(9223372036854775807n)
    expect((normalized.values[1].value as unknown as {value: bigint}).value).toBe(-9223372036854775808n)
    expect(normalized.values[2].enum_int_value).toBe(9007199254740993n)
    expect(raw.values[0].value).toBe('9223372036854775807')
    expect(fromIpc(normalized)).toBe(normalized)
    expect(fromIpc({ kind: 'object', value: { fields: { enum_int_value: { kind: 'int', value: '7' } } } }).value.fields.enum_int_value.value).toBe(7n)
    expect(() => fromIpc({ kind: 'int', value: 9007199254740992 })).toThrow('safe integer')
  })

  it('restores shared nested descriptors without merging instance enum values', () => {
    const transfer = {
      snapshot: { records: [
        { fields: [{ name: 'a', annotation: null }, { name: 'b', annotation: null }] },
        { fields: [{ name: 'a', annotation: null }, { name: 'missing', annotation: null }] },
      ] },
      annotations: [
        { metadata: { declared_type: 'Mode', enum_int_value: '9007199254740993' }, item: null, key: null, children: {} },
        { metadata: { declared_type: 'Mode', enum_int_value: '2' }, item: null, key: null, children: {} },
        { metadata: { declared_type: '[Mode]' }, item: 0, key: null, children: { '0': 0, '1': 1 } },
      ], annotation_ids: [[2, 2], [0, null]],
    } as unknown as FileRecordsTransfer
    const decoded = decodeFileRecords(fromIpc(transfer))
    const first = decoded.records[0].fields[0].annotation!
    expect(decoded.records[0].fields[1].annotation).toBe(first)
    expect(first.children!['0']).toBe(first.item_annotation)
    expect(decoded.records[1].fields[0].annotation).toBe(first.item_annotation)
    expect(first.children!['0']!.enum_int_value).toBe(9007199254740993n)
    expect(first.children!['1']!.enum_int_value).toBe(2n)
    expect(decoded.records[1].fields[1].annotation).toBeNull()
    expect(transfer.snapshot.records[0].fields[0].annotation).toBeNull()
  })
})
