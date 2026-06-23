//! JSON exporter for validated Coflow data models.
//!
//! This crate converts an already-built [`CfdDataModel`] into table-oriented
//! JSON values. It deliberately does not load files or run checks.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_api::{
    ArtifactContentKind, ArtifactFile, ArtifactSet, CfdDataModel, CftContainer, DataExporter,
    Diagnostic, DiagnosticSet, ExportContext, ExporterDescriptor, OutputSpec,
};
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder, ExportError};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonExportError {
    pub message: String,
}

impl JsonExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for JsonExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for JsonExportError {}

impl From<ExportError> for JsonExportError {
    fn from(error: ExportError) -> Self {
        Self::new(error.message)
    }
}

/// Converts every table in the data model into one JSON array value.
///
/// The returned map key is the CFT type/table name. Values are arrays whose
/// order follows the table's original record order in the data model.
///
/// # Errors
///
/// Returns an error when a model record or field cannot be matched back to the
/// compiled schema. A `CfdDataModel` built from the same `CftContainer` should
/// not hit these errors.
pub fn export_json_model(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Value>, JsonExportError> {
    export_model_with_encoder(schema, model, &mut JsonEncoder).map_err(JsonExportError::from)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonExporter;

pub const JSON_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
    id: "json",
    display_name: "JSON",
    table_file_extension: "json",
    content_kind: ArtifactContentKind::Json,
};

impl DataExporter for JsonExporter {
    fn descriptor(&self) -> &'static ExporterDescriptor {
        &JSON_EXPORTER_DESCRIPTOR
    }

    fn export(
        &self,
        ctx: ExportContext<'_>,
        _output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let tables = export_json_model(ctx.schema, ctx.model).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "JSON-EXPORT",
                "EXPORT",
                format!("failed to export JSON model: {err}"),
            ))
        })?;
        let files = tables
            .into_iter()
            .map(|(table, value)| ArtifactFile::json(format!("{table}.json"), value))
            .collect();
        Ok(ArtifactSet::new(files))
    }
}

struct JsonEncoder;

impl ExportEncoder for JsonEncoder {
    type Error = JsonExportError;
    type Value = Value;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Ok(Value::Null)
    }

    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        Ok(Value::Bool(value))
    }

    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> {
        Ok(Value::Number(Number::from(value)))
    }

    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| JsonExportError::new("cannot export non-finite float"))
    }

    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn array(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        Ok(Value::Array(values))
    }

    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        let mut object = Map::new();
        for (key, value) in entries {
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}
