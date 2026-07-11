//! `MessagePack` exporter for validated Coflow data models.
//!
//! This crate converts an already-built [`CfdDataModel`] into table-oriented
//! `MessagePack` bytes. Each table is encoded as a bare `MessagePack` array value.

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
    ArtifactContentKind, ArtifactFile, ArtifactSet, DataExporter, Diagnostic, DiagnosticSet,
    ExportContext, ExporterDescriptor, OutputSpec,
};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder, ExportError};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePackExportError {
    pub message: String,
}

impl MessagePackExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MessagePackExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for MessagePackExportError {}

impl From<ExportError> for MessagePackExportError {
    fn from(error: ExportError) -> Self {
        Self::new(error.message)
    }
}

/// Converts every table in the data model into one bare `MessagePack` array.
///
/// The returned map key is the CFT type/table name. Values are complete
/// `MessagePack` byte buffers with no additional file header, manifest, schema
/// hash, encryption, or checksum.
///
/// # Errors
///
/// Returns an error when a model record or field cannot be matched back to the
/// compiled schema, or when a value cannot be encoded as `MessagePack`.
pub fn export_messagepack_model(
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Vec<u8>>, MessagePackExportError> {
    export_model_with_encoder(schema, model, &mut MessagePackEncoder)
        .map_err(MessagePackExportError::from)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MessagePackExporter;

pub const MESSAGEPACK_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
    id: "messagepack",
    display_name: "MessagePack",
    table_file_extension: "msgpack",
    content_kind: ArtifactContentKind::Bytes,
};

impl DataExporter for MessagePackExporter {
    fn descriptor(&self) -> &'static ExporterDescriptor {
        &MESSAGEPACK_EXPORTER_DESCRIPTOR
    }

    fn export(
        &self,
        ctx: ExportContext<'_>,
        _output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let tables = export_messagepack_model(ctx.schema, ctx.model).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "MESSAGEPACK-EXPORT",
                "EXPORT",
                format!("failed to export MessagePack model: {err}"),
            ))
        })?;
        let files = tables
            .into_iter()
            .map(|(table, bytes)| ArtifactFile::bytes(format!("{table}.msgpack"), bytes))
            .collect();
        Ok(ArtifactSet::new(files))
    }
}

struct MessagePackEncoder;

impl MessagePackEncoder {
    fn len_as_u32(len: usize, kind: &str) -> Result<u32, MessagePackExportError> {
        u32::try_from(len).map_err(|_| {
            MessagePackExportError::new(format!(
                "cannot encode {kind} with {len} entries as MessagePack: length exceeds u32::MAX"
            ))
        })
    }
}

impl ExportEncoder for MessagePackEncoder {
    type Error = MessagePackExportError;
    type Value = Vec<u8>;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_nil(&mut bytes).map_err(encode_error)?;
        Ok(bytes)
    }

    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_bool(&mut bytes, value).map_err(encode_error)?;
        Ok(bytes)
    }

    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_sint(&mut bytes, value).map_err(encode_error)?;
        Ok(bytes)
    }

    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_f64(&mut bytes, value).map_err(encode_error)?;
        Ok(bytes)
    }

    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_str(&mut bytes, value).map_err(encode_error)?;
        Ok(bytes)
    }

    fn array(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_array_len(&mut bytes, Self::len_as_u32(values.len(), "array")?)
            .map_err(encode_error)?;
        for value in values {
            bytes.extend(value);
        }
        Ok(bytes)
    }

    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        let mut bytes = Vec::new();
        rmp::encode::write_map_len(&mut bytes, Self::len_as_u32(entries.len(), "map")?)
            .map_err(encode_error)?;
        for (key, value) in entries {
            rmp::encode::write_str(&mut bytes, &key).map_err(encode_error)?;
            bytes.extend(value);
        }
        Ok(bytes)
    }
}

fn encode_error(error: impl fmt::Display) -> MessagePackExportError {
    MessagePackExportError::new(error.to_string())
}
