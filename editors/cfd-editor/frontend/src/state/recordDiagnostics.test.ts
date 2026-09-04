import { describe, expect, it } from 'vitest'
import type { DiagnosticItem } from '../wire'
import {
  buildRecordDiagnosticIndex,
  diagnosticsForRecord,
  type RecordDiagnosticTarget,
} from './recordDiagnostics'

const target: RecordDiagnosticTarget = {
  filePath: 'data/items.cfd',
  coordinate: { actual_type: 'Item', key: 'one' },
}
const fallback = {
  fieldDiagnostics: [{ severity: 'error', field_path: 'name', message: 'stale' }],
  severity: 'error' as const,
}

function diagnostic(
  severity: string,
  fieldPath: string | null = null,
  actualType: string = target.coordinate.actual_type,
): DiagnosticItem {
  return {
    id: `${severity}:${fieldPath ?? 'record'}:${actualType}`,
    severity,
    code: 'CHECK-007',
    stage: 'CHECK',
    message: `${severity} diagnostic`,
    target: fieldPath === null
      ? { kind: 'record', file_path: target.filePath, coordinate: { actual_type: actualType, key: target.coordinate.key } }
      : { kind: 'table_field', file_path: target.filePath, coordinate: { actual_type: actualType, key: target.coordinate.key }, field_path: fieldPath },
    contexts: [],
  }
}

describe('record diagnostic index', () => {
  it('clears stale cached diagnostics for a newer clean generation', () => {
    const index = buildRecordDiagnosticIndex([target], [])
    expect(diagnosticsForRecord(index, target, fallback)).toEqual({
      fieldDiagnostics: [],
      severity: null,
    })
  })

  it('indexes the strongest severity and current field diagnostics', () => {
    const index = buildRecordDiagnosticIndex([target], [
      diagnostic('warning', 'name'),
      diagnostic('error', 'price'),
    ])
    expect(diagnosticsForRecord(index, target, fallback)).toEqual({
      fieldDiagnostics: [
        { severity: 'warning', field_path: 'name', message: 'warning diagnostic' },
        { severity: 'error', field_path: 'price', message: 'error diagnostic' },
      ],
      severity: 'error',
    })
  })

  it('does not expand a typed diagnostic to a sibling type with the same key', () => {
    const sibling: RecordDiagnosticTarget = {
      filePath: target.filePath,
      coordinate: { actual_type: 'Weapon', key: target.coordinate.key },
    }
    const index = buildRecordDiagnosticIndex([target, sibling], [diagnostic('warning')])
    expect(diagnosticsForRecord(index, target, fallback).severity).toBe('warning')
    expect(diagnosticsForRecord(index, sibling, fallback).severity).toBe(null)
  })

  it('uses cached diagnostics only before project diagnostics are available', () => {
    expect(diagnosticsForRecord(undefined, target, fallback)).toBe(fallback)
  })
})
