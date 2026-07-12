//! Streaming JSON exporter for validated Coflow data models.

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
    ArtifactContentKind, ArtifactFile, ArtifactSet, DataExporter, DecodedOutputOptions, Diagnostic,
    DiagnosticSet, ExportContext, ExporterDescriptor, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;
use coflow_exporter_core::{export_model_to_sink, ExportError, ExportEventSink};
use std::fmt;
use std::io::Write;

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
        Self::new(error.to_string())
    }
}

/// Encodes the model directly into one JSON text artifact per non-empty table.
///
/// # Errors
///
/// Returns an error carrying the record and field path that failed.
pub fn export_json_artifacts(
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> Result<ArtifactSet, JsonExportError> {
    let mut sink = JsonEventSink::default();
    export_model_to_sink(schema, model, &mut sink).map_err(JsonExportError::from)?;
    ArtifactSet::new(sink.files).map_err(|err| JsonExportError::new(err.to_string()))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonExporter;

#[derive(Debug)]
struct JsonOutputOptions;

pub const JSON_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
    id: "json",
    display_name: "JSON",
    table_file_extension: "json",
    content_kind: ArtifactContentKind::Json,
};

/// Declares the JSON exporter role implemented by this package.
///
/// # Errors
///
/// Returns an error if the package declares the exporter id more than once.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_exporter(JsonExporter)?;
    Ok(bundle)
}

impl DataExporter for JsonExporter {
    fn descriptor(&self) -> &'static ExporterDescriptor {
        &JSON_EXPORTER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        if options.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(DecodedOutputOptions::new("json", JsonOutputOptions))
        } else {
            Err(DiagnosticSet::one(Diagnostic::error(
                "JSON-OPTIONS",
                "EXPORT",
                "JSON exporter does not accept output options",
            )))
        }
    }

    fn export(
        &self,
        ctx: ExportContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<JsonOutputOptions>("json")?;
        export_json_artifacts(ctx.schema, ctx.model).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "JSON-EXPORT",
                "EXPORT",
                format!("failed to export JSON model: {err}"),
            ))
        })
    }
}

#[derive(Debug, Default)]
struct JsonEventSink {
    files: Vec<ArtifactFile>,
    table_name: Option<String>,
    table_records: usize,
    bytes: Vec<u8>,
    stack: Vec<JsonContainer>,
}

#[derive(Debug)]
enum JsonContainer {
    Array { items: usize },
    Map { items: usize, awaiting_value: bool },
}

impl JsonEventSink {
    fn before_value(&mut self) -> Result<(), JsonExportError> {
        enum Prefix {
            Root,
            Array { comma: bool, depth: usize },
            Map,
        }

        let depth = self.stack.len();
        let prefix = match self.stack.last_mut() {
            Some(JsonContainer::Array { items }) => {
                let comma = *items > 0;
                *items += 1;
                Prefix::Array { comma, depth }
            }
            Some(JsonContainer::Map { awaiting_value, .. }) => {
                if !*awaiting_value {
                    return Err(JsonExportError::new("JSON map value has no preceding key"));
                }
                *awaiting_value = false;
                Prefix::Map
            }
            None => Prefix::Root,
        };

        match prefix {
            Prefix::Root => {
                if !self.bytes.is_empty() {
                    return Err(JsonExportError::new("JSON stream has multiple root values"));
                }
            }
            Prefix::Array { comma, depth } => {
                if comma {
                    self.bytes.push(b',');
                }
                self.bytes.push(b'\n');
                self.write_indent(depth);
            }
            Prefix::Map => {}
        }
        Ok(())
    }

    fn write_indent(&mut self, depth: usize) {
        self.bytes.resize(self.bytes.len() + depth * 2, b' ');
    }

    fn end_array_value(&mut self) -> Result<(), JsonExportError> {
        let Some(JsonContainer::Array { items }) = self.stack.pop() else {
            return Err(JsonExportError::new(
                "JSON array end does not match open container",
            ));
        };
        if items > 0 {
            self.bytes.push(b'\n');
            self.write_indent(self.stack.len());
        }
        self.bytes.push(b']');
        Ok(())
    }

    fn end_map_value(&mut self) -> Result<(), JsonExportError> {
        let Some(JsonContainer::Map {
            items,
            awaiting_value,
        }) = self.stack.pop()
        else {
            return Err(JsonExportError::new(
                "JSON map end does not match open container",
            ));
        };
        if awaiting_value {
            return Err(JsonExportError::new("JSON map ended before its value"));
        }
        if items > 0 {
            self.bytes.push(b'\n');
            self.write_indent(self.stack.len());
        }
        self.bytes.push(b'}');
        Ok(())
    }

    fn write_string_value(&mut self, value: &str) -> Result<(), JsonExportError> {
        serde_json::to_writer(&mut self.bytes, value)
            .map_err(|err| JsonExportError::new(err.to_string()))
    }
}

impl ExportEventSink for JsonEventSink {
    type Error = JsonExportError;

    fn begin_table(&mut self, name: &str, records: usize) -> Result<(), Self::Error> {
        if self.table_name.is_some() {
            return Err(JsonExportError::new("JSON table stream is already open"));
        }
        self.table_name = Some(name.to_string());
        self.table_records = records;
        self.bytes.clear();
        self.stack.clear();
        self.begin_array(records)
    }

    fn end_table(&mut self) -> Result<(), Self::Error> {
        self.end_array()?;
        if !self.stack.is_empty() {
            return Err(JsonExportError::new(
                "JSON table ended with open containers",
            ));
        }
        let name = self
            .table_name
            .take()
            .ok_or_else(|| JsonExportError::new("JSON table stream is not open"))?;
        if self.table_records > 0 {
            let text = String::from_utf8(std::mem::take(&mut self.bytes))
                .map_err(|err| JsonExportError::new(err.to_string()))?;
            self.files
                .push(ArtifactFile::text(format!("{name}.json"), text));
        } else {
            self.bytes.clear();
        }
        Ok(())
    }

    fn begin_array(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.before_value()?;
        self.bytes.push(b'[');
        self.stack.push(JsonContainer::Array { items: 0 });
        Ok(())
    }

    fn end_array(&mut self) -> Result<(), Self::Error> {
        self.end_array_value()
    }

    fn begin_map(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.before_value()?;
        self.bytes.push(b'{');
        self.stack.push(JsonContainer::Map {
            items: 0,
            awaiting_value: false,
        });
        Ok(())
    }

    fn map_key(&mut self, key: &str) -> Result<(), Self::Error> {
        let depth = self.stack.len();
        let comma = match self.stack.last_mut() {
            Some(JsonContainer::Map {
                items,
                awaiting_value,
            }) => {
                if *awaiting_value {
                    return Err(JsonExportError::new("JSON map key has no preceding value"));
                }
                let comma = *items > 0;
                *items += 1;
                *awaiting_value = true;
                comma
            }
            _ => return Err(JsonExportError::new("JSON map key is outside a map")),
        };
        if comma {
            self.bytes.push(b',');
        }
        self.bytes.push(b'\n');
        self.write_indent(depth);
        self.write_string_value(key)?;
        self.bytes.extend_from_slice(b": ");
        Ok(())
    }

    fn end_map(&mut self) -> Result<(), Self::Error> {
        self.end_map_value()
    }

    fn null(&mut self) -> Result<(), Self::Error> {
        self.before_value()?;
        self.bytes.extend_from_slice(b"null");
        Ok(())
    }

    fn bool(&mut self, value: bool) -> Result<(), Self::Error> {
        self.before_value()?;
        self.bytes
            .extend_from_slice(if value { b"true" } else { b"false" });
        Ok(())
    }

    fn int(&mut self, value: i64) -> Result<(), Self::Error> {
        self.before_value()?;
        write!(&mut self.bytes, "{value}").map_err(|err| JsonExportError::new(err.to_string()))
    }

    fn float(&mut self, value: f64) -> Result<(), Self::Error> {
        self.before_value()?;
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| JsonExportError::new("cannot export non-finite float"))?;
        write!(&mut self.bytes, "{number}").map_err(|err| JsonExportError::new(err.to_string()))
    }

    fn string(&mut self, value: &str) -> Result<(), Self::Error> {
        self.before_value()?;
        self.write_string_value(value)
    }
}
