// Compatibility surface that hides the shape difference between the
// generated bindings (snake-cased `CfdValue` / `CfdDictKey` /
// `CfdPathSegment` with a `value` discriminator) and the editor's legacy
// `FieldValue` shape (camel-cased `kind` with inline `v` / `items` / etc.).
//
// The wire still ships `CfdValue` — `api.ts` translates at the boundary
// via `cfdValueFromWire` / `cfdValueToWire`. Everything else in the
// front-end keeps using the legacy `FieldValue` shape for now so this
// PR doesn't have to rewrite ~4 700 lines of UI in one shot.

import type { CfdValue } from './CfdValue'
import type { CfdDictKey } from './CfdDictKey'
import type { CfdPathSegment } from './CfdPathSegment'
import type { CfdRecord } from './CfdRecord'
import type { CfdPath as GeneratedCfdPath } from './CfdPath'
import type { CfdEnumValue } from './CfdEnumValue'
import type { FieldAnnotation as GeneratedFieldAnnotation } from './FieldAnnotation'
import type { FieldCell as GeneratedFieldCell } from './FieldCell'
import type { FileRecords as GeneratedFileRecords } from './FileRecords'
import type { FileTreeNode as GeneratedFileTreeNode } from './FileTreeNode'
import type { FlatDiagnostic } from './FlatDiagnostic'
import type { GraphData as GeneratedGraphData } from './GraphData'
import type { GraphEdge as GeneratedGraphEdge } from './GraphEdge'
import type { GraphNode as GeneratedGraphNode } from './GraphNode'
import type { RecordCoordinate } from './RecordCoordinate'
import type { RecordRow as GeneratedRecordRow } from './RecordRow'
import type { RefTarget } from './RefTarget'
import type { SourceCapabilities as GeneratedSourceCapabilities } from './SourceCapabilities'
import type { SpreadInfo as GeneratedSpreadInfo } from './SpreadInfo'
import type { WriteFieldOutcome as GeneratedWriteFieldOutcome } from './WriteFieldOutcome'
import type { InsertRecordOutcome as GeneratedInsertRecordOutcome } from './InsertRecordOutcome'
import type { DeleteRecordOutcome as GeneratedDeleteRecordOutcome } from './DeleteRecordOutcome'
import type { DeletedRecordSnapshot } from './DeletedRecordSnapshot'
import type { ProjectSnapshot as GeneratedProjectSnapshot } from './ProjectSnapshot'

// Re-export raw wire types so call sites can use them directly when they
// need the authoritative shape.
export type {
  CfdValue,
  CfdDictKey,
  CfdPathSegment,
  CfdRecord,
  CfdEnumValue,
  FlatDiagnostic,
  RecordCoordinate,
  RefTarget,
  DeletedRecordSnapshot,
}

/**
 * Editor's wire error. `kind` widens to include `'load'` because some legacy
 * code paths still produced it; the generated Rust enum no longer emits
 * `Load` so new callers should not depend on it.
 */
export interface EditorError {
  kind: 'session' | 'project' | 'load' | 'write' | 'not_found' | 'other'
  message: string
  diagnostics?: DiagnosticItem[]
}

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

export function errorDiagnostics(err: unknown): DiagnosticItem[] {
  if (err && typeof err === 'object' && 'diagnostics' in err) {
    const diags = (err as { diagnostics: unknown }).diagnostics
    if (Array.isArray(diags)) return diags.map(flatDiagnosticToItem)
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

/**
 * Wire diagnostic flattened into the shape the front-end's diagnostics
 * panel renders. Mirrors `coflow_api::FlatDiagnostic` 1:1.
 */
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

export interface WriteFieldOutcome {
  row: RecordRow
  diagnostics: DiagnosticItem[]
  /** When the write changed the record's `id`, the new coordinate the
   *  front-end should rebind any cached state to. */
  renamed?: RecordCoordinate | null
}

export interface InsertRecordOutcome {
  file_records: FileRecords
  diagnostics: DiagnosticItem[]
}

export interface DeleteRecordOutcome {
  file_records: FileRecords
  diagnostics: DiagnosticItem[]
  deleted_snapshot: FieldValue | null
  deleted_actual_type: string | null
}

export interface FieldCell {
  name: string
  value: FieldValue
  is_spread?: boolean
  spread_info?: SpreadInfo
}

export interface SpreadInfo {
  source_record_key: string
  source_record_type: string
  source_record_file?: string | null
  source_field_path: string[]
}

/**
 * Legacy front-end value shape. Tagged with a camel-cased `kind` and inline
 * data fields. `api.ts` converts to/from the wire `CfdValue` so this
 * type stays stable for component code while the refactor proceeds.
 */
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
  | { view: 'record'; file: string; recordKey: string; actualType?: string }
  | { view: 'graph'; file: string }

/* ------------------------------------------------------------------ */
/* Wire ↔ legacy translation                                          */
/* ------------------------------------------------------------------ */

export function flatDiagnosticToItem(d: FlatDiagnostic): DiagnosticItem {
  return {
    severity: d.severity as 'error' | 'warning' | 'info',
    code: d.code,
    stage: d.stage,
    message: d.message,
    file_path: d.file_path,
    record_key: d.record_key,
    field_path: d.field_path,
  }
}

function diagnosticsToItems(diags: FlatDiagnostic[] | undefined): DiagnosticItem[] {
  return (diags ?? []).map(flatDiagnosticToItem)
}

export function projectSnapshotFromWire(s: GeneratedProjectSnapshot): ProjectSnapshot {
  return {
    session_id: s.session_id,
    project_root: s.project_root,
    file_tree: fileTreeFromWire(s.file_tree),
    diagnostics: diagnosticsToItems(s.diagnostics),
  }
}

export function fileTreeFromWire(nodes: GeneratedFileTreeNode[]): FileTreeNode[] {
  return nodes.map((n) => ({
    name: n.name,
    path: n.path,
    is_dir: n.is_dir,
    in_sources: n.in_sources,
    children: fileTreeFromWire(n.children),
  }))
}

export function fileRecordsFromWire(fr: GeneratedFileRecords): FileRecords {
  return {
    file_path: fr.file_path,
    type_names: fr.type_names,
    records: fr.records.map(recordRowFromWire),
    capabilities: sourceCapabilitiesFromWire(fr.capabilities),
  }
}

export function sourceCapabilitiesFromWire(c: GeneratedSourceCapabilities): SourceCapabilities {
  return {
    provider_id: c.provider_id,
    can_edit_field: c.can_edit_field,
    can_edit_key: c.can_edit_key,
    can_insert_record: c.can_insert_record,
    can_delete_record: c.can_delete_record,
    is_remote: c.is_remote,
  }
}

export function recordRowFromWire(row: GeneratedRecordRow): RecordRow {
  return {
    key: row.coordinate.key,
    actual_type: row.coordinate.actual_type,
    fields: row.fields.map(fieldCellFromWire),
  }
}

export function fieldCellFromWire(cell: GeneratedFieldCell): FieldCell {
  const annotation = cell.annotation
  return {
    name: cell.name,
    value: cfdValueFromWire(cell.value, annotation),
    is_spread: annotation?.spread_info != null,
    spread_info: annotation?.spread_info
      ? spreadInfoFromWire(annotation.spread_info)
      : undefined,
  }
}

export function spreadInfoFromWire(s: GeneratedSpreadInfo): SpreadInfo {
  return {
    source_record_key: s.source.key,
    source_record_type: s.source.actual_type,
    source_record_file: s.source_record_file ?? null,
    source_field_path: s.source_field_path,
  }
}

/**
 * `cell` is the wire `FieldCell`; `annotation` carries `ref_target_file` /
 * `enum_int_value` that the legacy `FieldValue` inlined onto the value
 * itself. Bring them back during translation so downstream code that
 * reads e.g. `value.target_file` keeps working.
 */
export function cfdValueFromWire(
  value: CfdValue,
  annotation: GeneratedFieldAnnotation | null = null,
): FieldValue {
  switch (value.kind) {
    case 'null':
      return { kind: 'Null' }
    case 'bool':
      return { kind: 'Bool', v: value.value }
    case 'int':
      // `bigint` from ts-rs; UI uses number. Refs / dict keys never exceed
      // 2^53 in practice; cast back when sending to the wire.
      return { kind: 'Int', v: Number(value.value as unknown as bigint | number) }
    case 'float':
      return { kind: 'Float', v: value.value }
    case 'string':
      return { kind: 'Str', v: value.value }
    case 'enum':
      return {
        kind: 'Enum',
        enum_name: value.value.enum_name,
        variant: value.value.variant ?? String(value.value.value),
        int_value: Number(value.value.value as unknown as bigint | number),
      }
    case 'object': {
      const record = value.value
      return {
        kind: 'Object',
        actual_type: record.actual_type,
        fields: Object.entries(record.fields).map(([name, v]) => ({
          name,
          value: cfdValueFromWire(v as CfdValue),
          // Nested cells inherit no spread/ref-target annotation from the
          // parent annotation; only top-level cells carry one.
        })),
      }
    }
    case 'ref':
      return {
        kind: 'Ref',
        target_type: value.value.target_type,
        target_key: value.value.target_key,
        target_file: annotation?.ref_target_file ?? null,
      }
    case 'array':
      return {
        kind: 'Array',
        items: value.value.map((v) => cfdValueFromWire(v as CfdValue)),
      }
    case 'dict':
      return {
        kind: 'Dict',
        entries: value.value.map(([k, v]) => ({
          key: cfdDictKeyFromWire(k as CfdDictKey),
          value: cfdValueFromWire(v as CfdValue),
        })),
      }
  }
}

export function cfdDictKeyFromWire(key: CfdDictKey): DictKey {
  switch (key.kind) {
    case 'string':
      return { kind: 'Str', v: key.value }
    case 'int':
      return { kind: 'Int', v: Number(key.value as unknown as bigint | number) }
    case 'enum':
      return {
        kind: 'Enum',
        enum_name: key.value.enum_name,
        variant: key.value.variant ?? String(key.value.value),
        int_value: Number(key.value.value as unknown as bigint | number),
      }
  }
}

/**
 * Translate a legacy `FieldValue` back to the wire `CfdValue` used by
 * `write_field` / `insert_record`. The reverse of `cfdValueFromWire`.
 */
export function cfdValueToWire(value: FieldValue): CfdValue {
  switch (value.kind) {
    case 'Null':
      return { kind: 'null' }
    case 'Bool':
      return { kind: 'bool', value: value.v }
    case 'Int':
      // Tauri's JSON layer parses `bigint`-typed numerics from i64 fields.
      // Number is fine on the way out — serde_json accepts integer JSON.
      return { kind: 'int', value: value.v as unknown as bigint }
    case 'Float':
      return { kind: 'float', value: value.v }
    case 'Str':
      return { kind: 'string', value: value.v }
    case 'Enum':
      return {
        kind: 'enum',
        value: {
          enum_name: value.enum_name,
          variant: value.variant || null,
          value: value.int_value as unknown as bigint,
        },
      }
    case 'Object': {
      const fields: { [k: string]: CfdValue } = {}
      for (const cell of value.fields) {
        fields[cell.name] = cfdValueToWire(cell.value)
      }
      return {
        kind: 'object',
        value: {
          key: '',
          actual_type: value.actual_type,
          fields,
        },
      }
    }
    case 'Ref':
      return {
        kind: 'ref',
        value: { target_type: value.target_type, target_key: value.target_key },
      }
    case 'Array':
      return { kind: 'array', value: value.items.map(cfdValueToWire) }
    case 'Dict':
      return {
        kind: 'dict',
        value: value.entries.map((e) => [cfdDictKeyToWire(e.key), cfdValueToWire(e.value)]),
      }
  }
}

export function cfdDictKeyToWire(key: DictKey): CfdDictKey {
  switch (key.kind) {
    case 'Str':
      return { kind: 'string', value: key.v }
    case 'Int':
      return { kind: 'int', value: key.v as unknown as bigint }
    case 'Enum':
      return {
        kind: 'enum',
        value: {
          enum_name: key.enum_name,
          variant: key.variant || null,
          value: key.int_value as unknown as bigint,
        },
      }
  }
}

export function fieldPathSegmentToWire(segment: FieldPathSegment): CfdPathSegment {
  switch (segment.kind) {
    case 'field':
      return { kind: 'field', value: segment.name }
    case 'index':
      return { kind: 'index', value: segment.i }
  }
}

export function writeFieldOutcomeFromWire(o: GeneratedWriteFieldOutcome): WriteFieldOutcome {
  return {
    row: recordRowFromWire(o.row),
    diagnostics: diagnosticsToItems(o.diagnostics),
    renamed: o.renamed ?? null,
  }
}

export function insertRecordOutcomeFromWire(o: GeneratedInsertRecordOutcome): InsertRecordOutcome {
  return {
    file_records: fileRecordsFromWire(o.file_records),
    diagnostics: diagnosticsToItems(o.diagnostics),
  }
}

export function deleteRecordOutcomeFromWire(o: GeneratedDeleteRecordOutcome): DeleteRecordOutcome {
  return {
    file_records: fileRecordsFromWire(o.file_records),
    diagnostics: diagnosticsToItems(o.diagnostics),
    deleted_snapshot: deletedSnapshotToFieldValue(o.deleted_snapshot ?? null),
    deleted_actual_type: o.deleted_snapshot?.record.actual_type ?? null,
  }
}

function deletedSnapshotToFieldValue(snap: DeletedRecordSnapshot | null): FieldValue | null {
  if (!snap) return null
  const record = snap.record
  const fields: FieldCell[] = Object.entries(record.fields).map(([name, v]) => ({
    name,
    value: cfdValueFromWire(v as CfdValue),
  }))
  return { kind: 'Object', actual_type: record.actual_type, fields }
}

export function graphFromWire(g: GeneratedGraphData): GraphData {
  return {
    nodes: g.nodes.map(graphNodeFromWire),
    edges: g.edges.map(graphEdgeFromWire),
  }
}

function graphNodeFromWire(n: GeneratedGraphNode): GraphNode {
  return {
    id: `${n.file_path}::${n.coordinate.key}`,
    key: n.coordinate.key,
    actual_type: n.coordinate.actual_type,
    file_path: n.file_path,
    in_focus_file: n.in_focus_file,
    is_collapsed: n.is_collapsed,
    fields: n.fields.map(fieldCellFromWire),
  }
}

function graphEdgeFromWire(e: GeneratedGraphEdge): GraphEdge {
  // Reconstruct the legacy node-id strings the UI keys against.
  return {
    source: e.source.key,
    target: e.target.key,
    field_path: e.field_path,
  }
}

/* unused helper kept for path/CfdPath shape compatibility */
export type CfdPath = GeneratedCfdPath
