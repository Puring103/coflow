//! Schema-specific Protobuf contract and data exporter.

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

mod contract;
mod encode;
mod schema;

use coflow_api::{
    ArtifactContentKind, ArtifactFile, ArtifactSet, DataExporter, DecodedOutputOptions, Diagnostic,
    DiagnosticSet, ExportContext, ExporterDescriptor, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
use std::fmt;

pub const PROTOBUF_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
    id: "protobuf",
    display_name: "Protobuf",
    table_file_extension: "pb",
    content_kind: ArtifactContentKind::Bytes,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct ProtobufExporter;

#[derive(Debug)]
struct ProtobufOutputOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtobufExportError {
    message: String,
}

impl ProtobufExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProtobufExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ProtobufExportError {}

/// Generates the contract and all table payloads from the same schema/model snapshot.
///
/// # Errors
///
/// Returns an error if schema data cannot be represented by the generated contract.
pub fn export_protobuf_artifacts(
    schema: &CftSchema,
    model: &CfdDataModel,
) -> Result<ArtifactSet, ProtobufExportError> {
    let contract = contract::Contract::build(schema)?;
    let mut files = vec![ArtifactFile::text(
        "_schema/coflow.proto",
        schema::render_contract(&contract),
    )];
    files.extend(encode::encode_tables(&contract, schema, model)?);
    ArtifactSet::new(files).map_err(|error| ProtobufExportError::new(error.to_string()))
}

/// Declares the Protobuf exporter role.
///
/// # Errors
///
/// Returns an error if the exporter id conflicts within the bundle.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_exporter(ProtobufExporter)?;
    Ok(bundle)
}

impl DataExporter for ProtobufExporter {
    fn descriptor(&self) -> &'static ExporterDescriptor {
        &PROTOBUF_EXPORTER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        if options.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(DecodedOutputOptions::new("protobuf", ProtobufOutputOptions))
        } else {
            Err(DiagnosticSet::one(Diagnostic::error(
                "PROTOBUF-OPTIONS",
                "EXPORT",
                "Protobuf exporter does not accept output options",
            )))
        }
    }

    fn export(
        &self,
        ctx: ExportContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<ProtobufOutputOptions>("protobuf")?;
        export_protobuf_artifacts(ctx.schema, ctx.model).map_err(|error| {
            DiagnosticSet::one(Diagnostic::error(
                "PROTOBUF-EXPORT",
                "EXPORT",
                format!("failed to export Protobuf model: {error}"),
            ))
        })
    }
}
