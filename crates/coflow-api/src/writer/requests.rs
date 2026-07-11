use crate::{DiagnosticSet, ResolvedSource};
use coflow_cft::CompiledSchema;
use coflow_data_model::{CfdDataModel, CfdPathSegment, CfdValue, RecordOrigin};
use std::collections::BTreeMap;
use std::path::Path;

/// Canonical data-model path segment used by writers and host wire adapters.
pub type WriteFieldPathSegment = CfdPathSegment;

/// Request describing a single field write.
#[derive(Debug, Clone)]
pub struct WriteCellRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub field_path: &'a [WriteFieldPathSegment],
    /// Source-neutral new value, serialized to the source format by the writer.
    pub new_value: &'a CfdValue,
    /// Optional pre-resolved schema type for the record. Writers that produce
    /// typed source representations (e.g. CFD) use this for serialization.
    pub schema: &'a CompiledSchema,
    /// Original `ResolvedSource` that produced the record. Writers consult
    /// `source.options` to retrieve provider-specific configuration (Lark
    /// app credentials, alternate Excel sheet mappings, etc.).
    pub source: &'a ResolvedSource,
}

/// Request describing a new top-level record insertion.
#[derive(Debug, Clone)]
pub struct InsertRecordRequest<'a> {
    /// Target source that should receive the new record.
    pub source: &'a ResolvedSource,
    /// Target sheet/table name for table sources. Text writers may ignore it.
    pub sheet: Option<&'a str>,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub fields: &'a BTreeMap<String, CfdValue>,
    pub schema: &'a CompiledSchema,
}

/// Request describing a top-level record deletion.
#[derive(Debug, Clone)]
pub struct DeleteRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a ResolvedSource,
}

/// Request describing a top-level record key rename.
#[derive(Debug, Clone)]
pub struct RenameRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a ResolvedSource,
    pub schema: &'a CompiledSchema,
}

/// Request to rewrite reference tokens inside one source after a record key
/// rename.
///
/// Engines use this for source syntax that compiles away before the runtime
/// model is built, such as provider-local spread entries. Direct refs are
/// rewritten through [`crate::SourceWriter::write_field`] at the exact [`RefEdge`]
/// site.
#[derive(Debug, Clone)]
pub struct RewriteRecordReferencesRequest<'a> {
    pub source: &'a ResolvedSource,
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub targets: &'a [SpreadRewriteTarget],
    pub schema: &'a CompiledSchema,
}

/// A precise spread-source token to rewrite inside provider source syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadRewriteTarget {
    pub origin: RecordOrigin,
    pub record_key: String,
    pub actual_type: String,
    pub object_path: Vec<WriteFieldPathSegment>,
}

/// Outcome of a writer call: which records were actually touched (so the
/// session can recompute checks) and any informational diagnostics.
#[derive(Debug, Clone, Default)]
pub struct WriteOutcome {
    /// Origins of records whose backing source changed. The session uses these
    /// to re-load specific records and run incremental checks; an empty vec
    /// means the writer made no observable change.
    pub touched_record_origins: Vec<RecordOrigin>,
    pub inserted_record_origin: Option<RecordOrigin>,
    pub deleted_record_origin: Option<RecordOrigin>,
    /// Optional non-fatal diagnostics surfaced to the user.
    pub diagnostics: DiagnosticSet,
}

/// Context passed to writers. Mirrors [`crate::SourceLoadContext`] but for writes.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CompiledSchema,
    /// The current data model. Writers use it to resolve [`CfdRecordId`]s
    /// inside the request value (e.g. for ref serialization). May be `None`
    /// when running pre-flight on a value that hasn't been merged into the
    /// model yet.
    pub model: Option<&'a CfdDataModel>,
}
