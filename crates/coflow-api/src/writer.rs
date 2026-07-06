use crate::{CfdDataModel, CfdValue, CftContainer, Diagnostic, DiagnosticSet};
use crate::{CreateTableRequest, RecordOrigin, ResolvedSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One step in a field path used by writers and the wire protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFieldPathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

/// Static description of a writer provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub capabilities: WriterCapabilities,
}

/// Editing capabilities exposed to the front-end so the UI can grey out
/// disabled actions per source.
///
/// Lower-bounded by the writer's actual implementation; the front-end must
/// not assume a writer can do more than these flags claim.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriterCapabilities {
    pub provider_id: String,
    pub can_edit_field: bool,
    pub can_edit_key: bool,
    pub can_insert_record: bool,
    pub can_delete_record: bool,
    pub can_create_table: bool,
    pub requires_full_refresh_after_write: bool,
    pub is_remote: bool,
}

impl WriterCapabilities {
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: false,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            can_create_table: false,
            requires_full_refresh_after_write: false,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn local_full() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: true,
            can_delete_record: true,
            can_create_table: true,
            requires_full_refresh_after_write: true,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn remote_field_edit() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: false,
            can_delete_record: false,
            can_create_table: false,
            requires_full_refresh_after_write: true,
            is_remote: true,
        }
    }

    #[must_use]
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }
}

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
    pub schema: &'a CftContainer,
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
    pub schema: &'a CftContainer,
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
    pub schema: &'a CftContainer,
}

/// Request to rewrite reference tokens inside one source after a record key
/// rename.
///
/// Engines use this for source syntax that compiles away before the runtime
/// model is built, such as provider-local spread entries. Direct refs are
/// rewritten through [`DataWriter::write_field`] at the exact [`RefEdge`] site.
#[derive(Debug, Clone)]
pub struct RewriteRecordReferencesRequest<'a> {
    pub source: &'a ResolvedSource,
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub targets: &'a [SpreadRewriteTarget],
    pub schema: &'a CftContainer,
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

/// Context passed to writers. Mirrors [`crate::LoadContext`] but for writes.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CftContainer,
    /// The current data model. Writers use it to resolve [`CfdRecordId`]s
    /// inside the request value (e.g. for ref serialization). May be `None`
    /// when running pre-flight on a value that hasn't been merged into the
    /// model yet.
    pub model: Option<&'a CfdDataModel>,
}

/// Trait for source-specific writers that persist field edits.
///
/// Implementations dispatch on [`RecordOrigin`] to locate the cell/span, write
/// the new value to the source (file, remote API, ...), and report which
/// records were touched so the session can run incremental checks.
pub trait DataWriter: Send + Sync {
    fn descriptor(&self) -> &'static WriterDescriptor;

    /// Cheap pre-flight check: type matches, target file exists, etc. The
    /// default implementation does nothing.
    fn preflight(&self, _ctx: WriteContext<'_>, _request: &WriteCellRequest<'_>) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Persist a single field change.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the write cannot be performed (origin
    /// mismatch, missing file, transport error, schema-invalid value, etc.).
    fn write_field(
        &self,
        ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet>;

    /// Persist a new top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot insert records for this
    /// source or when the request cannot be represented by the source format.
    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support inserting records",
        )))
    }

    /// Create a table/sheet and write its header row.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot create tables for this
    /// source or when the provider rejects the requested sheet/header.
    fn create_table(
        &self,
        _ctx: WriteContext<'_>,
        _request: &CreateTableRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support creating tables",
        )))
    }

    /// Rename a top-level record key.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot rename keys for this source
    /// or when the existing source no longer matches the requested old key.
    fn rename_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support renaming record keys",
        )))
    }

    /// Rewrite source-level references to a renamed record key.
    ///
    /// The default implementation is a no-op because ordinary `CfdValue::Ref`
    /// locations are updated via [`DataWriter::write_field`]. Providers should
    /// override this when their source syntax contains references that do not
    /// survive as runtime refs.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the source cannot be read or updated.
    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
    }

    /// Delete a top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot delete records for this
    /// source or when the target no longer matches the requested record.
    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support deleting records",
        )))
    }
}
