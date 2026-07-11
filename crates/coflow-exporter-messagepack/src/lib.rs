//! Streaming MessagePack exporter for validated Coflow data models.

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
    ExportContext, ExporterDescriptor, OutputSpec, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;
use coflow_exporter_core::{export_model_to_sink, ExportError, ExportEventSink};
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
        Self::new(error.to_string())
    }
}

/// Encodes the model directly into one MessagePack byte artifact per table.
///
/// # Errors
///
/// Returns an error carrying the record and field path that failed.
pub fn export_messagepack_artifacts(
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> Result<ArtifactSet, MessagePackExportError> {
    let mut sink = MessagePackEventSink::default();
    export_model_to_sink(schema, model, &mut sink).map_err(MessagePackExportError::from)?;
    ArtifactSet::new(sink.files).map_err(|err| MessagePackExportError::new(err.to_string()))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MessagePackExporter;

pub const MESSAGEPACK_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
    id: "messagepack",
    display_name: "MessagePack",
    table_file_extension: "msgpack",
    content_kind: ArtifactContentKind::Bytes,
};

/// Declares the MessagePack exporter role implemented by this package.
///
/// # Errors
///
/// Returns an error if the package declares the exporter id more than once.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_exporter(MessagePackExporter)?;
    Ok(bundle)
}

impl DataExporter for MessagePackExporter {
    fn descriptor(&self) -> &'static ExporterDescriptor {
        &MESSAGEPACK_EXPORTER_DESCRIPTOR
    }

    fn export(
        &self,
        ctx: ExportContext<'_>,
        _output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        export_messagepack_artifacts(ctx.schema, ctx.model).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "MESSAGEPACK-EXPORT",
                "EXPORT",
                format!("failed to export MessagePack model: {err}"),
            ))
        })
    }
}

#[derive(Debug, Default)]
struct MessagePackEventSink {
    files: Vec<ArtifactFile>,
    table_name: Option<String>,
    bytes: Vec<u8>,
}

impl MessagePackEventSink {
    fn len_as_u32(len: usize, kind: &str) -> Result<u32, MessagePackExportError> {
        u32::try_from(len).map_err(|_| {
            MessagePackExportError::new(format!(
                "cannot encode {kind} with {len} entries as MessagePack: length exceeds u32::MAX"
            ))
        })
    }

    fn encode_error(error: impl fmt::Display) -> MessagePackExportError {
        MessagePackExportError::new(error.to_string())
    }
}

impl ExportEventSink for MessagePackEventSink {
    type Error = MessagePackExportError;

    fn begin_table(&mut self, name: &str, records: usize) -> Result<(), Self::Error> {
        if self.table_name.is_some() {
            return Err(MessagePackExportError::new(
                "MessagePack table stream is already open",
            ));
        }
        self.table_name = Some(name.to_string());
        self.bytes.clear();
        rmp::encode::write_array_len(&mut self.bytes, Self::len_as_u32(records, "table")?)
            .map(|_| ())
            .map_err(Self::encode_error)
    }

    fn end_table(&mut self) -> Result<(), Self::Error> {
        let name = self
            .table_name
            .take()
            .ok_or_else(|| MessagePackExportError::new("MessagePack table stream is not open"))?;
        self.files.push(ArtifactFile::bytes(
            format!("{name}.msgpack"),
            std::mem::take(&mut self.bytes),
        ));
        Ok(())
    }

    fn begin_array(&mut self, len: usize) -> Result<(), Self::Error> {
        rmp::encode::write_array_len(&mut self.bytes, Self::len_as_u32(len, "array")?)
            .map(|_| ())
            .map_err(Self::encode_error)
    }

    fn end_array(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn begin_map(&mut self, len: usize) -> Result<(), Self::Error> {
        rmp::encode::write_map_len(&mut self.bytes, Self::len_as_u32(len, "map")?)
            .map(|_| ())
            .map_err(Self::encode_error)
    }

    fn map_key(&mut self, key: &str) -> Result<(), Self::Error> {
        rmp::encode::write_str(&mut self.bytes, key).map_err(Self::encode_error)
    }

    fn end_map(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn null(&mut self) -> Result<(), Self::Error> {
        rmp::encode::write_nil(&mut self.bytes).map_err(Self::encode_error)
    }

    fn bool(&mut self, value: bool) -> Result<(), Self::Error> {
        rmp::encode::write_bool(&mut self.bytes, value).map_err(Self::encode_error)
    }

    fn int(&mut self, value: i64) -> Result<(), Self::Error> {
        rmp::encode::write_sint(&mut self.bytes, value)
            .map(|_| ())
            .map_err(Self::encode_error)
    }

    fn float(&mut self, value: f64) -> Result<(), Self::Error> {
        rmp::encode::write_f64(&mut self.bytes, value).map_err(Self::encode_error)
    }

    fn string(&mut self, value: &str) -> Result<(), Self::Error> {
        rmp::encode::write_str(&mut self.bytes, value).map_err(Self::encode_error)
    }
}
