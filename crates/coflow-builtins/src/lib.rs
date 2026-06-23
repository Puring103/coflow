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
    registry.register_loader(coflow_loader_excel::ExcelLoader)?;
    registry.register_loader(coflow_loader_lark::LarkSheetLoader::default())?;
    registry.register_loader(coflow_loader_cfd::CfdLoader)?;
    registry.register_writer(coflow_loader_excel::ExcelWriter::new())?;
    registry.register_writer(coflow_loader_lark::LarkSheetWriter::default())?;
    registry.register_writer(coflow_loader_cfd::CfdWriter::new())?;
    registry.register_exporter(coflow_exporter_json::JsonExporter)?;
    registry.register_exporter(coflow_exporter_messagepack::MessagePackExporter)?;
    registry.register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)?;
    Ok(())
}
