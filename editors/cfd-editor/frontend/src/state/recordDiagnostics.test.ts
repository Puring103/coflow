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
  actualType: string | null = target.coordinate.actual_type,
): DiagnosticItem {
  return {
    severity,
    code: 'CFD-CHECK-007',
    stage: 'CHECK',
    message: `${severity} diagnostic`,
    file_path: target.filePath,
    actual_type: actualType,
    record_key: target.coordinate.key,
    field_path: fieldPath,
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

  it('expands untyped diagnostics only to matching file and key targets', () => {
    const sibling: RecordDiagnosticTarget = {
      filePath: target.filePath,
      coordinate: { actual_type: 'Weapon', key: target.coordinate.key },
    }
    const index = buildRecordDiagnosticIndex([target, sibling], [diagnostic('warning', null, null)])
    expect(diagnosticsForRecord(index, target, fallback).severity).toBe('warning')
    expect(diagnosticsForRecord(index, sibling, fallback).severity).toBe('warning')
  })

  it('uses cached diagnostics only before project diagnostics are available', () => {
    expect(diagnosticsForRecord(undefined, target, fallback)).toBe(fallback)
  })
})
