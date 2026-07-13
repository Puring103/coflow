import { describe, expect, it } from 'vitest'
import type { GraphNodeView, DiagnosticItem } from '../wire'
import { graphNodeDiagnosticSeverity } from './GraphView.diagnostics'

const node = {
  id: 'Item::one',
  key: 'one',
  actual_type: 'Item',
  coordinate: { actual_type: 'Item', key: 'one' },
  file_path: 'data/items.cfd',
  in_focus_file: true,
  is_collapsed: false,
  fields: [],
  field_diagnostics: [],
  diagnostic_severity: 'error',
} as GraphNodeView

function diagnostic(
  severity: string,
  filePath = node.file_path,
  key = node.key,
): DiagnosticItem {
  return {
    severity,
    file_path: filePath,
    record_key: key,
    actual_type: node.actual_type,
  } as DiagnosticItem
}

describe('graph node diagnostics', () => {
  it('clears stale cached severity when the current generation has no diagnostic', () => {
    expect(graphNodeDiagnosticSeverity(node, [])).toBeNull()
  })

  it('uses the strongest current diagnostic for the matching record', () => {
    expect(graphNodeDiagnosticSeverity(node, [
      diagnostic('warning'),
      diagnostic('error', 'data/other.cfd'),
      diagnostic('error'),
    ])).toBe('error')
  })

  it('falls back to cached severity before project diagnostics are available', () => {
    expect(graphNodeDiagnosticSeverity(node, undefined)).toBe('error')
  })
})
