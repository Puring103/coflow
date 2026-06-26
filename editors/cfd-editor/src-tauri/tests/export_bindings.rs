//! Generate TypeScript bindings for the editor's wire types.
//!
//! Run with: `cargo test --features ts-export -p cfd-editor export_bindings`.
//! ts-rs registers each type's export function via `inventory`; calling
//! `<T as TS>::export_all()` on a sentinel type pulls the whole registry.

#[cfg(feature = "ts-export")]
#[test]
fn export_bindings() {
    use ts_rs::TS;
    coflow_data_model::CfdValue::export_all().expect("export CfdValue tree");
    coflow_data_model::CfdRecord::export_all().expect("export CfdRecord tree");
    coflow_data_model::CfdDictKey::export_all().expect("export CfdDictKey tree");
    coflow_data_model::CfdPath::export_all().expect("export CfdPath tree");
    coflow_data_model::CfdPathSegment::export_all().expect("export CfdPathSegment tree");
    coflow_api::FlatDiagnostic::export_all().expect("export FlatDiagnostic");
    coflow_engine::FileTreeNode::export_all().expect("export FileTreeNode");
    coflow_engine::DimensionInfo::export_all().expect("export DimensionInfo");
    coflow_engine::WriteOutcome::export_all().expect("export WriteOutcome");
    coflow_project::DimensionConfig::export_all().expect("export DimensionConfig");
}

#[cfg(not(feature = "ts-export"))]
#[test]
fn export_bindings_requires_feature() {
    // Without the `ts-export` feature the binding generator does nothing.
    // CI runs `cargo test --features ts-export -p cfd-editor export_bindings`.
}
