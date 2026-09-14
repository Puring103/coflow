//! 语言文档同步、诊断、补全与格式化命令。

use cfd_editor_core::EditorHost;
use tauri::State;

use super::run_host_command;
use crate::editor::{
    EditorError, FunctionDocumentState, LanguageCompletion, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, ProjectBootstrap,
};
use coflow_runtime::FlatDiagnostic;
#[tauri::command]
pub(crate) async fn read_source_text(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().read_source_text(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn highlight_source_snapshot(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<LanguageDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .highlight_source_snapshot(session_id, &file_path, &source)
    })
    .await
}

#[tauri::command]
pub(crate) async fn sync_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .sync_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[tauri::command]
pub(crate) async fn validate_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<FlatDiagnostic>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .validate_source_text(session_id, &file_path, &source)
    })
    .await
}

#[tauri::command]
pub(crate) async fn complete_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    position: LanguagePosition,
    host: State<'_, EditorHost>,
) -> Result<Vec<LanguageCompletion>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .complete_language_document(session_id, &file_path, &source, version, &position)
    })
    .await
}

#[tauri::command]
pub(crate) async fn format_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageFormattingResult, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .format_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[tauri::command]
pub(crate) async fn close_language_document(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .close_language_document(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn function_document(
    session_id: u32,
    source: String,
    body: Option<String>,
    host: State<'_, EditorHost>,
) -> Result<FunctionDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .function_document(session_id, &source, body.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .write_source_text(session_id, &file_path, &source)
    })
    .await
}
