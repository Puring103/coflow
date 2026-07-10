use crate::{Diagnostic, DiagnosticSet, ResolvedSource};
use coflow_cft::CftSchemaView;
use coflow_data_model::CfdValue;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct TableContext<'a> {
    pub project_root: &'a Path,
}

#[derive(Debug, Clone)]
pub struct CreateTableRequest<'a> {
    pub source: &'a ResolvedSource,
    pub sheet: &'a str,
    pub actual_type: &'a str,
    pub headers: &'a [String],
}

#[derive(Debug, Clone)]
pub struct SyncHeaderRequest<'a> {
    pub source: &'a ResolvedSource,
    pub sheet: Option<&'a str>,
    pub actual_type: &'a str,
    pub headers: &'a [String],
    pub schema: Option<&'a CftSchemaView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableOperationResult {
    pub headers: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub diagnostics: DiagnosticSet,
}

pub trait TableManager: Send + Sync {
    fn descriptor(&self) -> &'static TableManagerDescriptor;

    /// Create a new table/sheet and write its header row.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot create the target table or
    /// the request is invalid for the source.
    fn create_table(
        &self,
        _ctx: TableContext<'_>,
        _request: &CreateTableRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        Err(unsupported_table_operation("creating tables"))
    }

    /// Synchronize a table/sheet header with a schema-derived header.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot sync the target header or
    /// the request is invalid for the source.
    fn sync_header(
        &self,
        _ctx: TableContext<'_>,
        _request: &SyncHeaderRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        Err(unsupported_table_operation("syncing headers"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableManagerDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub file_extensions: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub addressing: TableAddressing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAddressing {
    Document,
    Sheet,
}

#[derive(Debug, Clone)]
pub struct DimensionSourceRequest<'a> {
    pub source: &'a ResolvedSource,
    pub entries: &'a [DimensionSourceEntry],
    pub variants: &'a [String],
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

pub trait DimensionSourceManager: Send + Sync {
    fn descriptor(&self) -> &'static DimensionSourceManagerDescriptor;

    fn source_options(&self, _request: &DimensionSourceOptionsRequest<'_>) -> Value {
        Value::Object(serde_json::Map::new())
    }

    /// Synchronize a generated dimension source while preserving configured
    /// player-authored variant values.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot parse, render, or write
    /// the backing source.
    fn sync_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        _request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        Err(unsupported_table_operation("syncing dimension sources"))
    }
}

#[derive(Debug, Clone)]
pub struct DimensionSourceOptionsRequest<'a> {
    pub sheet: &'a str,
    pub actual_type: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimensionSourceManagerDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
}

fn unsupported_table_operation(operation: &'static str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "TABLE-UNSUPPORTED",
        "TABLE",
        format!("table manager does not support {operation}"),
    ))
}
