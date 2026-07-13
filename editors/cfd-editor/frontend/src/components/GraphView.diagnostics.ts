import type { GraphNodeView, DiagnosticItem } from '../wire'
import { diagnosticMatchesCoordinate, diagnosticSeverity } from '../wire'

export function graphNodeDiagnosticSeverity(
  node: GraphNodeView,
  diagnostics: DiagnosticItem[] | undefined,
): 'error' | 'warning' | null {
  if (!diagnostics) {
    return node.diagnostic_severity === 'error' || node.diagnostic_severity === 'warning'
      ? node.diagnostic_severity
      : null
  }
  let strongest: 'warning' | null = null
  for (const diagnostic of diagnostics) {
    if (
      diagnostic.file_path !== node.file_path
      || !diagnosticMatchesCoordinate(diagnostic, node.coordinate)
    ) continue
    const severity = diagnosticSeverity(diagnostic.severity)
    if (severity === 'error') return 'error'
    if (severity === 'warning') strongest = 'warning'
  }
  return strongest
}
