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
    registry.register_bundle(default_provider_bundle()?)
}

/// Builds the complete default package bundle without mutating a registry.
///
/// # Errors
///
/// Returns an error when two provider packages declare the same role id.
pub fn default_provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = coflow_loader_excel::provider_bundle()?;
    bundle.merge(coflow_loader_csv::provider_bundle()?)?;
    bundle.merge(coflow_loader_cfd::provider_bundle()?)?;
    bundle.merge(coflow_exporter_json::provider_bundle()?)?;
    bundle.merge(coflow_exporter_messagepack::provider_bundle()?)?;
    bundle.merge(coflow_codegen_csharp::provider_bundle()?)?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::default_provider_registry;

    #[test]
    fn default_registry_selects_csharp_loaders_by_data_format() {
        let registry = default_provider_registry().expect("default providers");
        assert_eq!(
            registry
                .select_loader("csharp", "json", None)
                .expect("JSON loader")
                .descriptor()
                .id,
            "csharp-json"
        );
        assert_eq!(
            registry
                .select_loader("csharp", "messagepack", None)
                .expect("MessagePack loader")
                .descriptor()
                .id,
            "csharp-messagepack"
        );
        assert!(registry.select_loader("csharp", "yaml", None).is_none());
    }
}
