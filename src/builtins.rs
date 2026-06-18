use coflow_api::{ProviderRegistrationError, ProviderRegistry};

/// Creates the built-in provider registry used by the CLI.
///
/// # Errors
///
/// Returns an error if built-in providers declare duplicate ids. That indicates
/// a programming error in the built-in provider set, but is surfaced as a normal
/// startup error instead of panicking.
pub fn builtin_registry() -> Result<ProviderRegistry, ProviderRegistrationError> {
    let mut registry = ProviderRegistry::default();
    register_builtin_providers(&mut registry)?;
    Ok(registry)
}

/// Registers all built-in providers into an existing registry.
///
/// # Errors
///
/// Returns an error if the target registry already contains one of the built-in
/// provider ids, or if the built-in set itself contains duplicate ids.
pub fn register_builtin_providers(
    registry: &mut ProviderRegistry,
) -> Result<(), ProviderRegistrationError> {
    registry.register_loader(coflow_loader_excel::ExcelLoader)?;
    registry.register_loader(coflow_loader_lark::LarkSheetLoader::default())?;
    registry.register_loader(coflow_loader_cfd::CfdLoader)?;
    registry.register_exporter(coflow_exporter_json::JsonExporter)?;
    registry.register_exporter(coflow_exporter_messagepack::MessagePackExporter)?;
    registry.register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)?;
    Ok(())
}
