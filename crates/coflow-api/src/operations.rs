use crate::{DecodedSourceOptions, Diagnostic, DiagnosticSet, ResolvedSource};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdValue;
use std::collections::BTreeMap;
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
    pub schema: Option<&'a CompiledSchema>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableOperationResult {
    pub headers: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableHeaderOptions {
    pub sheet: String,
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub columns: BTreeMap<String, String>,
}

impl TableHeaderOptions {
    #[must_use]
    pub fn new(sheet: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            type_name: None,
            key: None,
            columns: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
        self
    }

    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn with_columns(
        mut self,
        columns: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.columns = columns
            .into_iter()
            .map(|(source, field)| (source.into(), field.into()))
            .collect();
        self
    }

    #[must_use]
    pub fn key_column(&self) -> &str {
        self.key.as_deref().unwrap_or("id")
    }

    #[must_use]
    pub fn field_headers(&self) -> BTreeMap<String, String> {
        self.columns
            .iter()
            .map(|(source, field)| (field.clone(), source.clone()))
            .collect()
    }
}

pub trait TableManager: Send + Sync {
    fn descriptor(&self) -> &'static TableManagerDescriptor;

    /// Return the configured record type for a table sheet, when the provider's
    /// source options define one.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when provider-specific table source options are
    /// malformed.
    fn type_for_sheet(
        &self,
        _source: &ResolvedSource,
        _sheet: Option<&str>,
    ) -> Result<Option<String>, DiagnosticSet> {
        Ok(None)
    }

    /// Return the configured table sheet for a record type, when provider
    /// source options define one.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when provider-specific table source options are
    /// malformed.
    fn sheet_for_type(
        &self,
        _source: &ResolvedSource,
        _actual_type: &str,
    ) -> Result<Option<String>, DiagnosticSet> {
        Ok(None)
    }

    /// Return provider-decoded table header options for a table sheet/type.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when provider-specific table source options are
    /// malformed.
    fn header_options(
        &self,
        _source: &ResolvedSource,
        sheet: &str,
        actual_type: &str,
    ) -> Result<TableHeaderOptions, DiagnosticSet> {
        Ok(TableHeaderOptions::new(sheet).with_type(actual_type))
    }

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

    fn source_options(
        &self,
        _request: &DimensionSourceOptionsRequest<'_>,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        Ok(DecodedSourceOptions::new(self.descriptor().id, ()))
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
