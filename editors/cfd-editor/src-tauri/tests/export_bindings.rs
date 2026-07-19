//! Generate TypeScript bindings for the editor's wire types.
//!
//! Run with: `cargo test --features ts-export -p cfd-editor export_bindings`.
//! ts-rs registers each type's export function via `inventory`; calling
//! `<T as TS>::export_all()` on a sentinel type pulls the whole registry.

#[cfg(feature = "ts-export")]
#[test]
fn export_bindings() {
    use cfd_editor_lib::editor::types as t;
    use ts_rs::TS;
    // Core types
    coflow_data_model::CfdValue::export_all().expect("export CfdValue tree");
    coflow_data_model::CfdRecord::export_all().expect("export CfdRecord tree");
    coflow_data_model::CfdDictKey::export_all().expect("export CfdDictKey tree");
    coflow_data_model::CfdPathSegment::export_all().expect("export CfdPathSegment tree");
    coflow_api::FlatDiagnostic::export_all().expect("export FlatDiagnostic");
    coflow_api::WriterCapabilities::export_all().expect("export WriterCapabilities");
    coflow_runtime::FileTreeNode::export_all().expect("export FileTreeNode");
    coflow_runtime::DimensionValueCoordinate::export_all()
        .expect("export DimensionValueCoordinate");
    coflow_runtime::DimensionValueView::export_all().expect("export DimensionValueView");
    coflow_runtime::CreateFieldSource::export_all().expect("export CreateFieldSource");
    coflow_runtime::CreateRequiredInput::export_all().expect("export CreateRequiredInput");
    // Editor composition views
    t::EditorError::export_all().expect("export EditorError");
    t::ProjectSnapshot::export_all().expect("export ProjectSnapshot");
    t::EditorProjectSettings::export_all().expect("export EditorProjectSettings");
    t::FileRecords::export_all().expect("export FileRecords");
    t::RecordRow::export_all().expect("export RecordRow");
    t::FieldCell::export_all().expect("export FieldCell");
    t::FieldAnnotation::export_all().expect("export FieldAnnotation");
    t::SpreadInfo::export_all().expect("export SpreadInfo");
    t::WriteFieldOutcome::export_all().expect("export WriteFieldOutcome");
    t::WriteDimensionValueOutcome::export_all().expect("export WriteDimensionValueOutcome");
    t::CollectionEdit::export_all().expect("export CollectionEdit");
    t::RenameRecordOutcome::export_all().expect("export RenameRecordOutcome");
    t::InsertRecordOutcome::export_all().expect("export InsertRecordOutcome");
    t::CreateRecordDraft::export_all().expect("export CreateRecordDraft");
    t::CreateRecordFieldDraft::export_all().expect("export CreateRecordFieldDraft");
    t::DeleteRecordOutcome::export_all().expect("export DeleteRecordOutcome");
    t::ReorderRecordsOutcome::export_all().expect("export ReorderRecordsOutcome");
    t::DeletedRecordSnapshot::export_all().expect("export DeletedRecordSnapshot");
    t::GraphData::export_all().expect("export GraphData");
    t::GraphNode::export_all().expect("export GraphNode");
    t::GraphEdge::export_all().expect("export GraphEdge");
    t::RefTarget::export_all().expect("export RefTarget");
}

#[cfg(not(feature = "ts-export"))]
#[test]
fn export_bindings_requires_feature() {
    // Without the `ts-export` feature the binding generator does nothing.
    // CI runs `cargo test --features ts-export -p cfd-editor export_bindings`.
}
