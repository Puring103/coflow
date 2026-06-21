// Hand-maintained bindings matching Rust types in coflow-editor-core/src/types.rs

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
}

export interface RecordRow {
  key: string
  actual_type: string
  fields: FieldCell[]
}

export interface FieldCell {
  name: string
  value: FieldValue
  /** Top-level field came from a `...spread` expansion; not editable here. */
  is_spread?: boolean
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
