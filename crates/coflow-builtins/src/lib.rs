//! Default provider registration shared by Coflow hosts.

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
#![allow(clippy::multiple_crate_versions)]

use coflow_api::{ProviderBundle, ProviderRegistrationError, ProviderRegistry};
use std::sync::Arc;

/// Creates the built-in provider registry used by host applications.
///
/// # Errors
///
/// Returns an error if built-in providers declare duplicate ids.
pub fn default_provider_registry() -> Result<ProviderRegistry, ProviderRegistrationError> {
    let mut registry = ProviderRegistry::default();
    register_default_providers(&mut registry)?;
    Ok(registry)
}

/// Registers all built-in providers into an existing registry.
///
/// # Errors
///
/// Returns an error if the target registry already contains one of the built-in
/// provider ids, or if the built-in set itself contains duplicate ids.
pub fn register_default_providers(
    registry: &mut ProviderRegistry,
) -> Result<(), ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    let excel_writer = Arc::new(coflow_loader_excel::ExcelWriter::new());
    let csv_writer = Arc::new(coflow_loader_csv::CsvWriter::new());
    let (lark_loader, lark_writer) = coflow_loader_lark::lark_provider_roles(
        coflow_loader_lark::UreqLarkHttpClient,
    );
    let lark_writer = Arc::new(lark_writer);
    let cfd_writer = Arc::new(coflow_loader_cfd::CfdWriter::new());

    bundle.add_source_provider(coflow_loader_excel::ExcelLoader)?;
    bundle.add_source_provider(coflow_loader_csv::CsvLoader)?;
    bundle.add_source_provider(lark_loader)?;
    bundle.add_source_provider(coflow_loader_cfd::CfdLoader)?;
    bundle.add_source_writer_arc(Arc::clone(&excel_writer))?;
    bundle.add_source_writer_arc(Arc::clone(&lark_writer))?;
    bundle.add_source_writer_arc(Arc::clone(&cfd_writer))?;
    bundle.add_source_writer_arc(Arc::clone(&csv_writer))?;
    bundle.add_table_manager_arc(Arc::clone(&excel_writer))?;
    bundle.add_table_manager_arc(Arc::clone(&csv_writer))?;
    bundle.add_table_manager_arc(Arc::clone(&cfd_writer))?;
    bundle.add_table_manager_arc(Arc::clone(&lark_writer))?;
    bundle.add_dimension_source_manager_arc(Arc::clone(&csv_writer))?;
    bundle.add_dimension_source_manager_arc(Arc::clone(&cfd_writer))?;
    bundle.add_exporter(coflow_exporter_json::JsonExporter)?;
    bundle.add_exporter(coflow_exporter_messagepack::MessagePackExporter)?;
    bundle.add_codegen(coflow_codegen_csharp::CsharpCodeGenerator)?;
    registry.register_bundle(bundle)
}
