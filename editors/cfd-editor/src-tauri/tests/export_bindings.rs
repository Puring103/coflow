//! Generate TypeScript bindings for the editor's wire types.
//!
//! Run with: `cargo test --features ts-export -p cfd-editor export_bindings`.
//! ts-rs registers each type's export function via `inventory`; calling
//! `<T as TS>::export_all()` on a sentinel type pulls the whole registry.

#[cfg(feature = "ts-export")]
#[test]
fn export_bindings() {
    use cfd_editor_lib::editor::types as t;
    // Core types
    export_with_retry::<coflow_data_model::CfdValue>("CfdValue tree");
    export_with_retry::<coflow_data_model::CfdRecord>("CfdRecord tree");
    export_with_retry::<coflow_data_model::CfdDictKey>("CfdDictKey tree");
    export_with_retry::<coflow_data_model::CfdPathSegment>("CfdPathSegment tree");
    export_with_retry::<coflow_api::FlatDiagnostic>("FlatDiagnostic");
    export_with_retry::<coflow_api::WriterCapabilities>("WriterCapabilities");
    export_with_retry::<coflow_runtime::FileTreeNode>("FileTreeNode");
    export_with_retry::<coflow_runtime::DimensionValueCoordinate>("DimensionValueCoordinate");
    export_with_retry::<coflow_runtime::DimensionValueView>("DimensionValueView");
    export_with_retry::<coflow_runtime::CreateFieldSource>("CreateFieldSource");
    export_with_retry::<coflow_runtime::CreateRequiredInput>("CreateRequiredInput");
    // Editor composition views
    export_with_retry::<t::EditorError>("EditorError");
    export_with_retry::<t::ProjectSnapshot>("ProjectSnapshot");
    export_with_retry::<t::PluginSchemaType>("PluginSchemaType");
    export_with_retry::<t::PluginSchemaField>("PluginSchemaField");
    export_with_retry::<t::EditorProjectSettings>("EditorProjectSettings");
    export_with_retry::<t::EditorRecordGroup>("EditorRecordGroup");
    export_with_retry::<t::ViewConfig>("ViewConfig");
    export_with_retry::<t::ViewKind>("ViewKind");
    export_with_retry::<t::FileRecords>("FileRecords");
    export_with_retry::<t::ProjectSearchMode>("ProjectSearchMode");
    export_with_retry::<t::ProjectSearchHit>("ProjectSearchHit");
    export_with_retry::<t::ProjectSearchResults>("ProjectSearchResults");
    export_with_retry::<t::RecordRow>("RecordRow");
    export_with_retry::<t::FieldCell>("FieldCell");
    export_with_retry::<t::FieldAnnotation>("FieldAnnotation");
    export_with_retry::<t::WriteFieldOutcome>("WriteFieldOutcome");
    export_with_retry::<t::WriteDimensionValueOutcome>("WriteDimensionValueOutcome");
    export_with_retry::<t::CollectionEdit>("CollectionEdit");
    export_with_retry::<t::RenameRecordOutcome>("RenameRecordOutcome");
    export_with_retry::<t::InsertRecordOutcome>("InsertRecordOutcome");
    export_with_retry::<t::CreateRecordDraft>("CreateRecordDraft");
    export_with_retry::<t::CreateRecordFieldDraft>("CreateRecordFieldDraft");
    export_with_retry::<t::DeleteRecordOutcome>("DeleteRecordOutcome");
    export_with_retry::<t::ReorderRecordsOutcome>("ReorderRecordsOutcome");
    export_with_retry::<t::DeletedRecordSnapshot>("DeletedRecordSnapshot");
    export_with_retry::<t::GraphData>("GraphData");
    export_with_retry::<t::GraphNode>("GraphNode");
    export_with_retry::<t::GraphEdge>("GraphEdge");
    export_with_retry::<t::RefTarget>("RefTarget");
    normalize_generated_bindings();
}

#[cfg(feature = "ts-export")]
fn export_with_retry<T: ts_rs::TS + 'static>(label: &str) {
    let mut delays = [10, 20, 40, 80, 160, 320, 640].into_iter();
    loop {
        match T::export_all() {
            Ok(()) => return,
            Err(error) => {
                let Some(delay_ms) = delays.next() else {
                    panic!("export {label}: {error}");
                };
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
        }
    }
}

#[cfg(feature = "ts-export")]
fn normalize_generated_bindings() {
    let bindings_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/bindings");
    let entries = std::fs::read_dir(&bindings_dir).expect("read generated bindings directory");

    for entry in entries {
        let path = entry.expect("read generated binding entry").path();
        if path.extension().is_none_or(|extension| extension != "ts") {
            continue;
        }

        let contents = std::fs::read_to_string(&path).expect("read generated binding");
        let normalized = contents
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        let normalized = if contents.ends_with('\n') {
            format!("{normalized}\n")
        } else {
            normalized
        };

        if normalized != contents {
            std::fs::write(path, normalized).expect("normalize generated binding");
        }
    }
}

#[cfg(not(feature = "ts-export"))]
#[test]
fn export_bindings_requires_feature() {
    // Without the `ts-export` feature the binding generator does nothing.
    // CI runs `cargo test --features ts-export -p cfd-editor export_bindings`.
}
