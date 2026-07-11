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

use coflow_api::{ProviderRegistrationError, ProviderRegistry};
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
    let excel_writer = Arc::new(coflow_loader_excel::ExcelWriter::new());
    let csv_writer = Arc::new(coflow_loader_csv::CsvWriter::new());
    let lark_writer = Arc::new(coflow_loader_lark::LarkSheetWriter::default());
    let cfd_writer = Arc::new(coflow_loader_cfd::CfdWriter::new());

    registry.register_source_provider(coflow_loader_excel::ExcelLoader)?;
    registry.register_source_provider(coflow_loader_csv::CsvLoader)?;
    registry.register_source_provider(coflow_loader_lark::LarkSheetLoader::default())?;
    registry.register_source_provider(coflow_loader_cfd::CfdLoader)?;
    registry.register_source_writer_arc(Arc::clone(&excel_writer))?;
    registry.register_source_writer_arc(Arc::clone(&lark_writer))?;
    registry.register_source_writer_arc(Arc::clone(&cfd_writer))?;
    registry.register_source_writer_arc(Arc::clone(&csv_writer))?;
    registry.register_table_manager_arc(Arc::clone(&excel_writer))?;
    registry.register_table_manager_arc(Arc::clone(&csv_writer))?;
    registry.register_table_manager_arc(Arc::clone(&cfd_writer))?;
    registry.register_table_manager_arc(Arc::clone(&lark_writer))?;
    registry.register_dimension_source_manager_arc(Arc::clone(&csv_writer))?;
    registry.register_dimension_source_manager_arc(Arc::clone(&cfd_writer))?;
    registry.register_exporter(coflow_exporter_json::JsonExporter)?;
    registry.register_exporter(coflow_exporter_messagepack::MessagePackExporter)?;
    registry.register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)?;
    Ok(())
}
