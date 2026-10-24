//! Export host-independent editor DTOs and their dependencies.
//! Run: cargo test --features ts-export -p cfd-editor-core `export_bindings`

#[cfg(feature = "ts-export")]
#[path = "../../test-support/bindings.rs"]
mod support;

#[cfg(feature = "ts-export")]
#[test]
fn export_bindings() {
    use cfd_editor_core::editor::types as t;
    use support::export_type;
    export_type::<coflow_runtime::CfdValue>("CfdValue tree");
    export_type::<coflow_runtime::CfdRecord>("CfdRecord tree");
    export_type::<coflow_runtime::CfdDictKey>("CfdDictKey tree");
    export_type::<coflow_runtime::CfdPathSegment>("CfdPathSegment tree");
    export_type::<coflow_runtime::FlatDiagnostic>("FlatDiagnostic");
    export_type::<coflow_runtime::WriterCapabilities>("WriterCapabilities");
    export_type::<coflow_runtime::FileTreeNode>("FileTreeNode");
    export_type::<coflow_runtime::DimensionValueCoordinate>("DimensionValueCoordinate");
    export_type::<coflow_runtime::DimensionValueView>("DimensionValueView");
    export_type::<coflow_runtime::DimensionInfo>("DimensionInfo");
    export_type::<coflow_runtime::CreateFieldSource>("CreateFieldSource");
    export_type::<coflow_runtime::CreateRequiredInput>("CreateRequiredInput");
    export_type::<coflow_runtime::ProjectDiff>("ProjectDiff tree");
    export_type::<t::EditorError>("EditorError");
    export_type::<t::ProjectBootstrap>("ProjectBootstrap");
    export_type::<t::PluginSchemaType>("PluginSchemaType");
    export_type::<t::PluginSchemaField>("PluginSchemaField");
    export_type::<t::LanguagePosition>("LanguagePosition");
    export_type::<t::LanguageRange>("LanguageRange");
    export_type::<t::LanguageTextEdit>("LanguageTextEdit");
    export_type::<t::LanguageFormattingResult>("LanguageFormattingResult");
    export_type::<t::LanguageDiagnostic>("LanguageDiagnostic");
    export_type::<t::LanguageDocumentState>("LanguageDocumentState");
    export_type::<t::LanguageCompletion>("LanguageCompletion");
    export_type::<t::EditorProjectSettings>("EditorProjectSettings");
    export_type::<t::DimensionFileRecords>("DimensionFileRecords");
    export_type::<t::DimensionFileRow>("DimensionFileRow");
    export_type::<t::EditorRecordGroup>("EditorRecordGroup");
    export_type::<t::ViewConfig>("ViewConfig");
    export_type::<t::ViewKind>("ViewKind");
    export_type::<t::FileRecords>("FileRecords");
    export_type::<t::ProjectSearchMode>("ProjectSearchMode");
    export_type::<t::ProjectSearchHit>("ProjectSearchHit");
    export_type::<t::ProjectSearchResults>("ProjectSearchResults");
    export_type::<t::RecordRow>("RecordRow");
    export_type::<t::FieldCell>("FieldCell");
    export_type::<t::FieldAnnotation>("FieldAnnotation");
    export_type::<t::WriteFieldOutcome>("WriteFieldOutcome");
    export_type::<t::WriteDimensionValueOutcome>("WriteDimensionValueOutcome");
    export_type::<t::CollectionEdit>("CollectionEdit");
    export_type::<t::RenameRecordOutcome>("RenameRecordOutcome");
    export_type::<t::InsertRecordOutcome>("InsertRecordOutcome");
    export_type::<t::CreateRecordDraft>("CreateRecordDraft");
    export_type::<t::CreateRecordFieldDraft>("CreateRecordFieldDraft");
    export_type::<t::DeleteRecordOutcome>("DeleteRecordOutcome");
    export_type::<t::ReorderRecordsOutcome>("ReorderRecordsOutcome");
    export_type::<t::DeletedRecordSnapshot>("DeletedRecordSnapshot");
    export_type::<t::GraphData>("GraphData");
    export_type::<t::GraphNode>("GraphNode");
    export_type::<t::GraphEdge>("GraphEdge");
    export_type::<t::RefTarget>("RefTarget");
    export_type::<cfd_editor_core::ProjectReloadedPayload>("ProjectReloadedEvent");
    export_type::<cfd_editor_core::ProjectWatchErrorPayload>("ProjectWatchErrorEvent");
    support::normalize_generated_bindings(env!("CARGO_MANIFEST_DIR"));
}
