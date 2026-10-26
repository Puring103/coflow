//! 数据查询、展示转换与 mutation 命令。

use cfd_editor_core::EditorHost;
use tauri::State;

use super::run_host_command;
use cfd_editor_core::editor::{
    BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit, CreateRecordDraft,
    DeleteRecordOutcome, EditorError, EnumVariantOption, FileRecordsTransfer, GraphData,
    GraphQuery, InsertRecordOutcome, PluginSchemaType, ProjectSearchMode, ProjectSearchResults,
    RecordRow, RefTarget, RenameRecordOutcome, ReorderRecordsOutcome, WriteDimensionValueOutcome,
    WriteFieldOutcome,
};
use coflow_runtime::{
    CfdPathSegment, CfdValue, DimensionValueCoordinate, DimensionValueView, RecordCoordinate,
};

#[tauri::command]
pub(crate) async fn get_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<FileRecordsTransfer, EditorError> {
    run_host_command("get_file_records", host, move |host| {
        host.sessions()
            .get_file_records(session_id, &file_path)
            .map(Into::into)
    })
    .await
}

#[tauri::command]
pub(crate) async fn search_records(
    session_id: u32,
    query: String,
    mode: ProjectSearchMode,
    limit: usize,
    host: State<'_, EditorHost>,
) -> Result<ProjectSearchResults, EditorError> {
    run_host_command("search_records", host, move |host| {
        host.sessions()
            .search_records(session_id, &query, mode, limit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_plugin_schema(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<Vec<PluginSchemaType>, EditorError> {
    run_host_command("get_plugin_schema", host, move |host| {
        host.sessions().get_plugin_schema(session_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_plugin_records_by_type(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RecordRow>, EditorError> {
    run_host_command("get_plugin_records_by_type", host, move |host| {
        host.sessions()
            .get_plugin_records_by_type(session_id, &type_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_graph(
    session_id: u32,
    file_path: String,
    depth: Option<usize>,
    limit: Option<usize>,
    host: State<'_, EditorHost>,
) -> Result<GraphData, EditorError> {
    run_host_command("get_graph", host, move |host| {
        host.sessions().get_graph(
            session_id,
            &GraphQuery {
                file_path,
                depth,
                limit,
            },
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<EnumVariantOption>, EditorError> {
    run_host_command("get_enum_variants", host, move |host| {
        host.sessions().get_enum_variants(session_id, &enum_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_ref_targets(
    session_id: u32,
    target_type: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RefTarget>, EditorError> {
    run_host_command("get_ref_targets", host, move |host| {
        host.sessions().get_ref_targets(session_id, &target_type)
    })
    .await
}

#[tauri::command]
pub(crate) async fn make_default_object(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    run_host_command("make_default_object", host, move |host| {
        host.sessions().make_default_object(session_id, &type_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_record_draft(
    session_id: u32,
    actual_type: String,
    host: State<'_, EditorHost>,
) -> Result<CreateRecordDraft, EditorError> {
    run_host_command("create_record_draft", host, move |host| {
        host.sessions()
            .create_record_draft(session_id, &actual_type)
    })
    .await
}

#[tauri::command]
pub(crate) async fn render_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command("render_cell_text", host, move |host| {
        host.sessions()
            .render_cell_text(session_id, &coordinate, &field_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn parse_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    text: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    run_host_command("parse_cell_text", host, move |host| {
        host.sessions()
            .parse_cell_text(session_id, &coordinate, &field_path, &text)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_field(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    new_value: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    run_host_command("write_field", host, move |host| {
        host.sessions()
            .write_field(session_id, &coordinate, &field_path, &new_value)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_fields(
    session_id: u32,
    writes: Vec<BatchWriteFieldInput>,
    host: State<'_, EditorHost>,
) -> Result<BatchWriteFieldOutcome, EditorError> {
    run_host_command("write_fields", host, move |host| {
        host.sessions().write_fields(session_id, &writes)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DimensionValueView, EditorError> {
    run_host_command("get_dimension_value", host, move |host| {
        host.sessions().get_dimension_value(session_id, &coordinate)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    expected_value: coflow_runtime::DimensionValueState,
    new_value: coflow_runtime::DimensionValueState,
    destination_file: Option<String>,
    host: State<'_, EditorHost>,
) -> Result<WriteDimensionValueOutcome, EditorError> {
    run_host_command("write_dimension_value", host, move |host| {
        host.sessions().write_dimension_value(
            session_id,
            &coordinate,
            &expected_value,
            &new_value,
            destination_file.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn edit_collection(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    edit: CollectionEdit,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    run_host_command("edit_collection", host, move |host| {
        host.sessions()
            .edit_collection(session_id, &coordinate, &field_path, edit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn insert_record(
    session_id: u32,
    file_path: String,
    record_id: String,
    actual_type: String,
    fields: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<InsertRecordOutcome, EditorError> {
    run_host_command("insert_record", host, move |host| {
        host.sessions()
            .insert_record(session_id, &file_path, &record_id, &actual_type, fields)
    })
    .await
}

#[tauri::command]
pub(crate) async fn rename_record_id(
    session_id: u32,
    coordinate: RecordCoordinate,
    new_id: String,
    host: State<'_, EditorHost>,
) -> Result<RenameRecordOutcome, EditorError> {
    run_host_command("rename_record_id", host, move |host| {
        host.sessions()
            .rename_record_id(session_id, &coordinate, &new_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DeleteRecordOutcome, EditorError> {
    run_host_command("delete_record", host, move |host| {
        host.sessions().delete_record(session_id, &coordinate)
    })
    .await
}

#[tauri::command]
pub(crate) async fn swap_records(
    session_id: u32,
    first: RecordCoordinate,
    second: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command("swap_records", host, move |host| {
        host.sessions().swap_records(session_id, &first, &second)
    })
    .await
}

#[tauri::command]
pub(crate) async fn move_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command("move_record", host, move |host| {
        host.sessions()
            .move_record(session_id, &coordinate, target_index)
    })
    .await
}

#[tauri::command]
pub(crate) async fn transfer_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    destination_file: String,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command("transfer_record", host, move |host| {
        host.sessions()
            .transfer_record(session_id, &coordinate, &destination_file, target_index)
    })
    .await
}

#[tauri::command]
pub(crate) async fn apply_mutation_batch(
    session_id: u32,
    ops: Vec<coflow_runtime::MutationOp>,
    host: State<'_, EditorHost>,
) -> Result<cfd_editor_core::editor::BatchMutationOutcome, EditorError> {
    run_host_command("apply_mutation_batch", host, move |host| {
        host.sessions().apply_mutation_batch(session_id, ops)
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_linked_record(
    session_id: u32,
    input: cfd_editor_core::editor::CreateLinkedRecordInput,
    host: State<'_, EditorHost>,
) -> Result<cfd_editor_core::editor::LinkedRecordOutcome, EditorError> {
    run_host_command("create_linked_record", host, move |host| {
        host.sessions().create_linked_record(session_id, input)
    })
    .await
}
