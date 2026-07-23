import type { FieldDiagnostic } from '../bindings/FieldDiagnostic'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import {
  coordinateId,
  diagnosticDisplayMessage,
  diagnosticSeverity,
  type DiagnosticItem,
} from '../wire'

export interface RecordDiagnosticTarget {
  filePath: string
  coordinate: RecordCoordinate
}

export interface RecordDiagnosticProjection {
  fieldDiagnostics: FieldDiagnostic[]
  severity: 'error' | 'warning' | null
}

export type RecordDiagnosticIndex = Map<string, RecordDiagnosticProjection> | undefined

export function buildRecordDiagnosticIndex(
  targets: readonly RecordDiagnosticTarget[],
  diagnostics: DiagnosticItem[] | undefined,
): RecordDiagnosticIndex {
  if (!diagnostics) return undefined
  const targetsByAnchor = new Map<string, RecordDiagnosticTarget[]>()
  for (const target of targets) {
    const anchor = diagnosticAnchor(target.filePath, target.coordinate.key)
    const existing = targetsByAnchor.get(anchor)
    if (existing) existing.push(target)
    else targetsByAnchor.set(anchor, [target])
  }

  const index = new Map<string, RecordDiagnosticProjection>()
  for (const diagnostic of diagnostics) {
    if (!diagnostic.file_path || !diagnostic.record_key) continue
    const candidates = targetsByAnchor.get(
      diagnosticAnchor(diagnostic.file_path, diagnostic.record_key),
    )
    if (!candidates) continue
    for (const target of candidates) {
      if (
        diagnostic.actual_type !== null
        && diagnostic.actual_type !== target.coordinate.actual_type
      ) continue
      const key = recordDiagnosticKey(target.filePath, target.coordinate)
      const projection = index.get(key) ?? {
        fieldDiagnostics: [],
        severity: null,
      }
      const severity = diagnosticSeverity(diagnostic.severity)
      if (severity === 'error' || (severity === 'warning' && projection.severity === null)) {
        projection.severity = severity
      }
      if (diagnostic.field_path) {
        projection.fieldDiagnostics.push({
          severity,
          field_path: diagnostic.field_path,
          message: diagnosticDisplayMessage(diagnostic),
        })
      }
      index.set(key, projection)
    }
  }
  return index
}

export function diagnosticsForRecord(
  index: RecordDiagnosticIndex,
  target: RecordDiagnosticTarget,
  fallback: RecordDiagnosticProjection,
): RecordDiagnosticProjection {
  if (!index) return fallback
  return index.get(recordDiagnosticKey(target.filePath, target.coordinate)) ?? {
    fieldDiagnostics: [],
    severity: null,
  }
}

function diagnosticAnchor(filePath: string, recordKey: string): string {
  return `${filePath}\u001f${recordKey}`
}

function recordDiagnosticKey(filePath: string, coordinate: RecordCoordinate): string {
  return `${filePath}\u001f${coordinateId(coordinate)}`
}
