use crate::data_model::{CfdValue, DimensionValueDraft};
use crate::{CfdSource, Diagnostic, DiagnosticSet};
use coflow_language::{
    CftDimension, CftField, CftSchema, CftType, FieldName, RecordKey, VariantName,
};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct CfdWriteContext<'a> {
    pub project_root: &'a Path,
}

#[derive(Debug, Clone)]
pub struct DimensionSourceRequest<'a> {
    pub source: &'a CfdSource,
    pub entries: &'a [DimensionSourceEntry],
    pub variants: &'a [String],
}

#[derive(Debug, Clone, Copy)]
pub struct DimensionSourceSchema<'a> {
    pub schema: &'a CftSchema,
    pub dimension: &'a CftDimension,
    pub source_type: &'a CftType,
    pub source_field: &'a CftField,
}

#[derive(Debug, Clone)]
pub struct DimensionSourceLoadRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionSourceSchema<'a>,
    pub singleton_source_fields: &'a [FieldName],
    pub validate_singleton_shape: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DimensionSourceLoadResult {
    pub values: Vec<DimensionValueDraft>,
}

#[derive(Debug, Clone)]
pub struct WriteDimensionValueRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionSourceSchema<'a>,
    pub source_key: &'a RecordKey,
    pub variant: &'a VariantName,
    pub new_value: Option<&'a CfdValue>,
}

#[derive(Debug, Clone)]
pub struct RewriteDimensionRecordRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionSourceSchema<'a>,
    pub old_key: &'a RecordKey,
    pub new_key: Option<&'a RecordKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionSourceEntry {
    pub key: String,
    pub actual_type: String,
    pub default: CfdValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DimensionSourceResult {
    pub changed: bool,
}

pub trait CfdDimensionWriter: Send + Sync {
    fn descriptor(&self) -> &'static CfdDimensionWriterDescriptor;

    /// Load managed variant values directly into record-owned overlay inputs.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the source cannot be parsed against the
    /// canonical field schema.
    fn load_dimension_source(
        &self,
        _ctx: CfdWriteContext<'_>,
        _request: &DimensionSourceLoadRequest<'_>,
    ) -> Result<DimensionSourceLoadResult, DiagnosticSet> {
        Err(unsupported_dimension_operation("loading dimension sources"))
    }

    /// Write or clear one variant value in a managed dimension source.
    ///
    /// `None` clears the physical value so the overlay becomes missing;
    /// `Some(CfdValue::Null)` stores an explicit null.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the coordinate is stale or cannot be written.
    fn write_dimension_value(
        &self,
        _ctx: CfdWriteContext<'_>,
        _request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        Err(unsupported_dimension_operation("writing dimension values"))
    }

    /// Rename or delete one owner-record row while preserving its variants.
    /// `None` deletes the row; `Some` replaces its record key.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the managed row cannot be rewritten.
    fn rewrite_dimension_record(
        &self,
        _ctx: CfdWriteContext<'_>,
        _request: &RewriteDimensionRecordRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        Err(unsupported_dimension_operation(
            "rewriting dimension records",
        ))
    }

    /// Synchronize a generated dimension source while preserving configured
    /// player-authored variant values.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot parse, render, or write
    /// the backing source.
    fn sync_dimension_source(
        &self,
        _ctx: CfdWriteContext<'_>,
        _request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        Err(unsupported_dimension_operation("syncing dimension sources"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfdDimensionWriterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
}

fn unsupported_dimension_operation(operation: &'static str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "DIMENSION-UNSUPPORTED",
        "DIMENSION",
        format!("CFD dimension writer does not support {operation}"),
    ))
}
