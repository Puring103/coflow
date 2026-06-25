// Hand-maintained bindings matching Rust types in src-tauri/src/editor/types.rs

/**
 * Structured error returned by every Tauri command. The editor surfaces
 * `message` in the banner and pushes `diagnostics` (when present) into the
 * diagnostics panel without any string parsing.
 */
export interface EditorError {
  kind: 'session' | 'project' | 'load' | 'write' | 'not_found' | 'other'
  message: string
  diagnostics?: DiagnosticItem[]
}

/**
 * Render any error returned from Tauri (whether a structured EditorError or
 * a stringified one from older code paths) to a single human-readable
 * message. Robust to legacy strings so the call sites don't have to branch.
 */
export function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err
  if (err && typeof err === 'object' && 'message' in err) {
    const msg = (err as { message: unknown }).message
    if (typeof msg === 'string') return msg
  }
  try {
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}

/**
 * Extract the structured diagnostics array from an EditorError if present;
 * returns an empty array otherwise.
 */
export function errorDiagnostics(err: unknown): DiagnosticItem[] {
  if (err && typeof err === 'object' && 'diagnostics' in err) {
    const diags = (err as { diagnostics: unknown }).diagnostics
    if (Array.isArray(diags)) return diags as DiagnosticItem[]
  }
  return []
}

export interface ProjectSnapshot {
  session_id: number
  project_root: string
  file_tree: FileTreeNode[]
  diagnostics: DiagnosticItem[]
}

export interface FileTreeNode {
  name: string
  path: string
  is_dir: boolean
  in_sources: boolean
  children: FileTreeNode[]
}

export interface DiagnosticItem {
  severity: 'error' | 'warning' | 'info'
  code: string
  stage: string
  message: string
  file_path: string | null
  record_key: string | null
  field_path: string | null
}

export interface FileRecords {
  file_path: string
  type_names: string[]
  records: RecordRow[]
  capabilities: SourceCapabilities
}

export interface SourceCapabilities {
  provider_id: string
  can_edit_field: boolean
  can_edit_key: boolean
  can_insert_record: boolean
  can_delete_record: boolean
  is_remote: boolean
}

export interface RecordRow {
  key: string
  actual_type: string
  fields: FieldCell[]
}

/**
 * Result returned by the `write_field` Tauri command. Bundles the
 * refreshed row with the project's full diagnostic set after the
 * post-write rebuild — every successful edit reruns the checker, so any
 * check failures introduced or resolved by the edit show up in
 * `diagnostics` without a follow-up query.
 */
export interface WriteFieldOutcome {
  row: RecordRow
  diagnostics: DiagnosticItem[]
}

/**
 * Result returned by `insert_record` / `delete_record`. Bundles the refreshed
 * `FileRecords` for the host file with the project's diagnostics after the
 * post-write rebuild — same shape as `WriteFieldOutcome` but the whole file's
 * row list changes (not just one row).
 */
export interface InsertRecordOutcome {
  file_records: FileRecords
  diagnostics: DiagnosticItem[]
}

export interface DeleteRecordOutcome {
  file_records: FileRecords
  diagnostics: DiagnosticItem[]
  /** Authoritative snapshot of the deleted record (Object FieldValue) for
   *  undo to re-insert. Absent only when the record could not be located
   *  before deletion. */
  deleted_snapshot: FieldValue | null
  deleted_actual_type: string | null
}

export interface FieldCell {
  name: string
  value: FieldValue
  /** True when this cell's value was inherited via a `...spread`. Mirrors
   *  `spread_info != null` for legacy callers; new code should consult
   *  `spread_info` for the source coordinates. */
  is_spread?: boolean
  /** Where this cell's value originally came from when it was inherited
   *  via a spread. Front-end uses it to render the cell as inherited and
   *  show a "jump to source" affordance. */
  spread_info?: SpreadInfo
  /** Marker for cells the user must not edit (e.g. localization `id` and
   *  `default` columns). The editor still renders the value, but disables
   *  inline editing. Defaults to false for regular records. */
  read_only?: boolean
}

export interface SpreadInfo {
  source_record_key: string
  source_record_type: string
  source_record_file?: string | null
  source_field_path: string[]
}

export type FieldValue =
  | { kind: 'Null' }
  | { kind: 'Bool'; v: boolean }
  | { kind: 'Int'; v: number }
  | { kind: 'Float'; v: number }
  | { kind: 'Str'; v: string }
  | { kind: 'Enum'; enum_name: string; variant: string; int_value: number }
  | { kind: 'Object'; actual_type: string; fields: FieldCell[] }
  | { kind: 'Ref'; target_type: string; target_key: string; target_file: string | null }
  | { kind: 'Array'; items: FieldValue[] }
  | { kind: 'Dict'; entries: DictEntry[] }

export interface DictEntry {
  key: DictKey
  value: FieldValue
}

export type DictKey =
  | { kind: 'Str'; v: string }
  | { kind: 'Int'; v: number }
  | { kind: 'Enum'; enum_name: string; variant: string; int_value: number }

export interface GraphData {
  nodes: GraphNode[]
  edges: GraphEdge[]
}

export interface GraphNode {
  id: string
  key: string
  actual_type: string
  file_path: string
  in_focus_file: boolean
  is_collapsed: boolean
  fields: FieldCell[]
}

export interface GraphEdge {
  source: string
  target: string
  field_path: string
}

export type FieldPathSegment =
  | { kind: 'field'; name: string }
  | { kind: 'index'; i: number }

export type Route =
  | { view: 'table'; file: string; typeFilter?: string }
  | { view: 'record'; file: string; recordKey: string }
  | { view: 'graph'; file: string }
