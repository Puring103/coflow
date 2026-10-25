#![allow(
    clippy::multiple_crate_versions,
    clippy::needless_pass_by_value,
    clippy::unreachable
)]

use std::sync::Arc;

mod commands;
mod events;
mod open;
mod plugin_manifest;
mod plugins;

use cfd_editor_core::EditorHost;
use tauri::Manager;

use commands::{
    add_project_input, build_project, build_project_status, check_project, close_language_document,
    close_session, complete_language_document, create_project_file, create_record_draft,
    delete_project_entry, delete_record, edit_collection, format_language_document,
    get_dimension_file_records, get_dimension_value, get_enum_variants, get_file_records,
    get_graph, get_plugin_records_by_type, get_plugin_schema, get_project_diff,
    get_project_settings, get_ref_targets, highlight_source_snapshot, init_project, insert_record,
    load_project, make_default_object, move_record, open_source_file, parse_cell_text,
    read_source_text, reload_session, rename_record_id, render_cell_text, search_records,
    set_default_table_column_widths, set_graph_compact_mode, set_graph_positions,
    set_record_groups, set_short_name_field, set_view_column_widths, set_view_order, set_views,
    set_workspace, swap_records, sync_language_document, transfer_record, validate_source_text,
    write_dimension_value, write_field, write_fields, write_source_text,
};
use events::TauriEditorEventSink;
use plugins::{
    install_frontend_plugin, install_project_frontend_plugin, list_frontend_plugins,
    list_project_frontend_plugins, set_project_frontend_plugin_enabled, uninstall_frontend_plugin,
    uninstall_project_frontend_plugin,
};

pub use plugins::{
    FrontendPluginBundle, FrontendPlugins, PluginScope, ProjectFrontendPlugins,
    ProjectPluginDefaults,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Start the CFD editor Tauri application.
///
/// # Errors
/// Returns a Tauri error if application setup, context generation, or the
/// runtime event loop fails to start.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // 桌面剪贴板通过原生接口访问，不触发 WebView 的站点授权弹窗。
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let events = Arc::new(TauriEditorEventSink::new(app.handle().clone()));
            let host = EditorHost::new(events).map_err(|err| err.to_string())?;
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            init_project,
            close_session,
            reload_session,
            add_project_input,
            create_project_file,
            delete_project_entry,
            get_project_settings,
            get_dimension_file_records,
            set_default_table_column_widths,
            set_graph_positions,
            set_graph_compact_mode,
            set_views,
            set_view_order,
            set_short_name_field,
            set_view_column_widths,
            set_record_groups,
            set_workspace,
            check_project,
            build_project,
            build_project_status,
            get_project_diff,
            open_source_file,
            read_source_text,
            sync_language_document,
            highlight_source_snapshot,
            validate_source_text,
            complete_language_document,
            format_language_document,
            close_language_document,
            write_source_text,
            get_file_records,
            search_records,
            get_plugin_schema,
            get_plugin_records_by_type,
            get_graph,
            get_enum_variants,
            get_ref_targets,
            make_default_object,
            create_record_draft,
            render_cell_text,
            parse_cell_text,
            write_field,
            write_fields,
            get_dimension_value,
            write_dimension_value,
            edit_collection,
            insert_record,
            rename_record_id,
            delete_record,
            swap_records,
            move_record,
            transfer_record,
            install_frontend_plugin,
            list_frontend_plugins,
            uninstall_frontend_plugin,
            install_project_frontend_plugin,
            list_project_frontend_plugins,
            uninstall_project_frontend_plugin,
            set_project_frontend_plugin_enabled,
        ])
        .run(tauri::generate_context!())
}
