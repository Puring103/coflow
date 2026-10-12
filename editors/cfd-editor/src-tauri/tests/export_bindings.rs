//! Export the desktop host's plugin DTOs; editor DTOs are exported by cfd-editor-core.

#[cfg(feature = "ts-export")]
#[path = "../../test-support/bindings.rs"]
mod support;

#[cfg(feature = "ts-export")]
#[test]
fn export_bindings() {
    use support::export_type;
    export_type::<cfd_editor_lib::FrontendPluginBundle>("FrontendPluginBundle");
    export_type::<cfd_editor_lib::FrontendPlugins>("FrontendPluginState");
    export_type::<cfd_editor_lib::ProjectPluginDefaults>("ProjectPluginDefaults");
    export_type::<cfd_editor_lib::ProjectFrontendPlugins>("FrontendPluginProjectState");
    support::normalize_generated_bindings(env!("CARGO_MANIFEST_DIR"));
}
