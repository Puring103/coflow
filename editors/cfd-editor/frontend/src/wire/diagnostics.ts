import type { EditorError } from '../bindings/EditorError'
import type { FlatDiagnostic } from '../bindings/FlatDiagnostic'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { topLevelFieldName } from './paths'

export type DiagnosticItem = FlatDiagnostic

export function diagnosticMatchesCoordinate(
  diagnostic: DiagnosticItem,
  coordinate: RecordCoordinate,
): boolean {
  const target = diagnosticRecordTarget(diagnostic)
  return !!target
    && target.coordinate.id === coordinate.id
    && target.coordinate.actual_type === coordinate.actual_type
}

export function diagnosticRecordTarget(diagnostic: DiagnosticItem) {
  return diagnostic.target.kind === 'table_field' || diagnostic.target.kind === 'record'
    ? diagnostic.target
    : null
}

export function diagnosticFilePath(diagnostic: DiagnosticItem): string | null {
  return diagnostic.target.kind === 'none' ? null : diagnostic.target.file_path
}

export function diagnosticFieldPath(diagnostic: DiagnosticItem): string | null {
  return diagnostic.target.kind === 'table_field' ? diagnostic.target.field_path : null
}

export function diagnosticSeverity(severity: string): 'error' | 'warning' | 'info' {
  return severity === 'error' || severity === 'warning' ? severity : 'info'
}

export function diagnosticDisplayMessage(diagnostic: DiagnosticItem): string {
  const lines = [diagnostic.message]
  for (const context of diagnostic.contexts ?? []) {
    let detail = context.kind
    if (context.kind === 'check' && context.name) detail = `check ${context.name}`
    else if (context.kind === 'when' && context.expression) detail = `在 when ${context.expression} 内`
    else if (context.kind === 'quantifier' && context.binding && context.item) {
      detail = `绑定 ${context.binding} 位于 ${context.item}`
    } else if (context.kind === 'dimension' && context.dimension && context.variant) {
      detail = `${context.dimension}=${context.variant}`
    }
    lines.push(`上下文: ${detail}`)
  }
  return lines.join('\n')
}

/** Stable identity for a diagnostic. Same anchor + code + message ⇒ same key,
 *  so a focus request survives project snapshot refreshes even without an
 *  explicit ID field on FlatDiagnostic. */
export function diagnosticKey(diagnostic: DiagnosticItem): string {
  return diagnostic.id
}

/** Compare a diagnostic against a (record, field?) anchor. When `fieldPath`
 *  is provided, matches only when the diagnostic sits at that path or shares
 *  the same top-level segment (so a cell/row angle-badge whose column is the
 *  top-level field lights up for any nested problem inside). */
export function diagnosticMatchesAnchor(
  diagnostic: DiagnosticItem,
  filePath: string,
  recordId: string,
  actualType: string | null,
  fieldPath: string | null,
): boolean {
  const target = diagnosticRecordTarget(diagnostic)
  if (!target || target.file_path !== filePath) return false
  if (target.coordinate.id !== recordId) return false
  if (actualType !== null && target.coordinate.actual_type !== actualType) {
    return false
  }
  if (fieldPath === null) return true
  const diagnosticPath = diagnosticFieldPath(diagnostic)
  if (!diagnosticPath) return false
  if (diagnosticPath === fieldPath) return true
  return topLevelFieldName(diagnosticPath) === topLevelFieldName(fieldPath)
}

/** 错误路由以后端 `kind` 判别器为准，形状推断只做兜底。 */
export function isEditorError(err: unknown): err is EditorError {
  if (!err || typeof err !== 'object') return false
  if ('kind' in err && typeof (err as { kind?: unknown }).kind === 'string') return true
  return (
    'message' in err &&
    'diagnostics' in err &&
    Array.isArray((err as { diagnostics?: unknown }).diagnostics)
  )
}

export function errorMessage(err: unknown): string {
  if (isEditorError(err)) {
    const state = err.publication?.state
    const guidance = state === 'partial' || state === 'unknown'
      ? '文件恢复未完成，请在日志与反馈中检查受影响文件和保留备份，重新加载后再编辑。'
      : err.kind === 'committed' ? '内容已保存，但项目刷新失败，请重新加载项目，不要重复执行写入。'
      : state === 'restored' ? '本次已写入的文件已恢复，可处理原因后重试。' : ''
    return [err.message, guidance, err.error_id ? `错误编号：${err.error_id}` : ''].filter(Boolean).join(' ')
  }
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try {
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}

export function errorDiagnostics(err: unknown): DiagnosticItem[] {
  return isEditorError(err) ? err.diagnostics : []
}
