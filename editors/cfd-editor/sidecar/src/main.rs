use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cfd_editor_core::editor::{
    BatchWriteFieldInput, CollectionEdit, EditorError, EditorWorkspaceState, GraphQuery,
    ProjectSearchMode, ViewConfig,
};
use cfd_editor_core::{EditorEvent, EditorEventSink, EditorHost};
use coflow_runtime::{
    CfdPathSegment, CfdValue, DimensionValueCoordinate, DimensionValueState, RecordCoordinate,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
struct Request {
    id: u64,
    command: String,
    #[serde(default)]
    args: Map<String, Value>,
}

#[derive(Debug, Serialize)]
struct Response<'a> {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a EditorError>,
}

#[derive(Debug, Serialize)]
struct EventMessage<'a, T> {
    event: &'a str,
    payload: T,
}

#[derive(Clone)]
struct ProtocolEventSink {
    output: Arc<Mutex<BufWriter<std::io::Stdout>>>,
}

impl std::fmt::Debug for ProtocolEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolEventSink").finish_non_exhaustive()
    }
}

impl EditorEventSink for ProtocolEventSink {
    fn emit(&self, event: EditorEvent) {
        let result = match event {
            EditorEvent::ProjectChanged(payload) => {
                write_message(&self.output, &EventMessage {
                    event: "project_changed",
                    payload,
                })
            }
            EditorEvent::ProjectWatchError(payload) => {
                write_message(&self.output, &EventMessage {
                    event: "project_watch_error",
                    payload,
                })
            }
        };
        if let Err(error) = result {
            eprintln!("failed to emit editor event: {error}");
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cfd-editor-sidecar failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), EditorError> {
    let output = Arc::new(Mutex::new(BufWriter::new(std::io::stdout())));
    let events = Arc::new(ProtocolEventSink {
        output: Arc::clone(&output),
    });
    let host = EditorHost::new(events)?;
    let input = BufReader::new(std::io::stdin());

    for line in input.lines() {
        let line = line.map_err(|error| EditorError::other(format!("failed to read request: {error}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("ignored invalid sidecar request: {error}");
                continue;
            }
        };
        let id = request.id;
        let result = dispatch(&host, &request.command, request.args);
        match result {
            Ok(value) => write_message(
                &output,
                &Response {
                    id,
                    result: Some(value),
                    error: None,
                },
            )?,
            Err(error) => write_message(
                &output,
                &Response {
                    id,
                    result: None,
                    error: Some(&error),
                },
            )?,
        }
    }
    Ok(())
}

fn write_message<T: Serialize>(
    output: &Arc<Mutex<BufWriter<std::io::Stdout>>>,
    message: &T,
) -> Result<(), EditorError> {
    let mut output = output
        .lock()
        .map_err(|_| EditorError::session("sidecar output lock poisoned"))?;
    serde_json::to_writer(&mut *output, message)
        .map_err(|error| EditorError::other(format!("failed to encode response: {error}")))?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|error| EditorError::other(format!("failed to write response: {error}")))
}

fn dispatch(
    host: &EditorHost,
    command: &str,
    args: Map<String, Value>,
) -> Result<Value, EditorError> {
    let sessions = host.sessions();
    macro_rules! arg {
        ($name:literal, $ty:ty) => {
            argument::<$ty>(&args, $name)?
        };
    }
    macro_rules! result {
        ($value:expr) => {
            encode($value?)
        };
    }

    match command {
        "ping" => Ok(Value::String("pong".to_string())),
        "load_project" => result!(host.load_project(Path::new(&arg!("yamlPath", String)))),
        "init_project" => result!(host.init_project(Path::new(&arg!("dir", String)))),
        "close_session" => result!(host.close_session(arg!("sessionId", u32))),
        "get_project_settings" => result!(sessions.get_project_settings(arg!("sessionId", u32))),
        "get_project_dimensions" => {
            result!(sessions.get_project_dimensions(arg!("sessionId", u32)))
        }
        "get_dimension_file_records" => result!(sessions.get_dimension_file_records(
            arg!("sessionId", u32),
            &arg!("filePath", String),
        )),
        "set_default_table_column_widths" => result!(sessions.set_default_table_column_widths(
            arg!("sessionId", u32),
            arg!("filePath", String),
            arg!("actualType", String),
            arg!("widths", std::collections::BTreeMap<String, f64>),
        )),
        "set_views" => result!(sessions.set_views(
            arg!("sessionId", u32),
            arg!("filePath", String),
            arg!("actualType", String),
            arg!("views", Vec<ViewConfig>),
        )),
        "set_view_column_widths" => result!(sessions.set_view_column_widths(
            arg!("sessionId", u32),
            arg!("filePath", String),
            arg!("actualType", String),
            arg!("viewId", String),
            arg!("widths", std::collections::BTreeMap<String, f64>),
        )),
        "set_record_groups" => result!(sessions.set_record_groups(
            arg!("sessionId", u32),
            arg!("filePath", String),
            arg!("actualType", String),
            arg!("groups", Vec<cfd_editor_core::editor::EditorRecordGroup>),
        )),
        "set_workspace" => result!(sessions.set_workspace(
            arg!("sessionId", u32),
            arg!("workspace", EditorWorkspaceState),
        )),
        "check_project" => result!(sessions.check_project(arg!("sessionId", u32))),
        "build_project" => result!(sessions.build_project(arg!("sessionId", u32))),
        "build_project_status" => {
            result!(sessions.build_project_status(arg!("sessionId", u32)))
        }
        "open_source_file" => {
            let path = sessions.source_file_path(
                arg!("sessionId", u32),
                &arg!("filePath", String),
            )?;
            open_with_default_application(&path)?;
            encode(())
        }
        "get_file_records" => result!(sessions.get_file_records(
            arg!("sessionId", u32),
            &arg!("filePath", String),
        )),
        "search_records" => result!(sessions.search_records(
            arg!("sessionId", u32),
            &arg!("query", String),
            arg!("mode", ProjectSearchMode),
            arg!("limit", usize),
        )),
        "get_plugin_schema" => result!(sessions.get_plugin_schema(arg!("sessionId", u32))),
        "get_plugin_records_by_type" => result!(sessions.get_plugin_records_by_type(
            arg!("sessionId", u32),
            &arg!("typeName", String),
        )),
        "get_graph" => result!(sessions.get_graph(
            arg!("sessionId", u32),
            &GraphQuery {
                file_path: arg!("filePath", String),
                depth: optional_argument(&args, "depth")?,
                limit: optional_argument(&args, "limit")?,
            },
        )),
        "get_enum_variants" => result!(sessions.get_enum_variants(
            arg!("sessionId", u32),
            &arg!("enumName", String),
        )),
        "get_ref_targets" => result!(sessions.get_ref_targets(
            arg!("sessionId", u32),
            &arg!("targetType", String),
        )),
        "make_default_object" => result!(sessions.make_default_object(
            arg!("sessionId", u32),
            &arg!("typeName", String),
        )),
        "create_record_draft" => result!(sessions.create_record_draft(
            arg!("sessionId", u32),
            &arg!("actualType", String),
        )),
        "render_cell_text" => result!(sessions.render_cell_text(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("fieldPath", Vec<CfdPathSegment>),
        )),
        "parse_cell_text" => result!(sessions.parse_cell_text(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("fieldPath", Vec<CfdPathSegment>),
            &arg!("text", String),
        )),
        "write_field" => result!(sessions.write_field(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("fieldPath", Vec<CfdPathSegment>),
            &arg!("newValue", CfdValue),
        )),
        "write_fields" => result!(sessions.write_fields(
            arg!("sessionId", u32),
            &arg!("writes", Vec<BatchWriteFieldInput>),
        )),
        "get_dimension_value" => result!(sessions.get_dimension_value(
            arg!("sessionId", u32),
            &arg!("coordinate", DimensionValueCoordinate),
        )),
        "write_dimension_value" => result!(sessions.write_dimension_value(
            arg!("sessionId", u32),
            &arg!("coordinate", DimensionValueCoordinate),
            &arg!("expectedValue", DimensionValueState),
            &arg!("newValue", DimensionValueState),
        )),
        "edit_collection" => result!(sessions.edit_collection(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("fieldPath", Vec<CfdPathSegment>),
            arg!("edit", CollectionEdit),
        )),
        "insert_record" => result!(sessions.insert_record(
            arg!("sessionId", u32),
            &arg!("filePath", String),
            &arg!("recordKey", String),
            &arg!("actualType", String),
            arg!("fields", CfdValue),
        )),
        "rename_record_key" => result!(sessions.rename_record_key(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("newKey", String),
        )),
        "delete_record" => result!(sessions.delete_record(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
        )),
        "swap_records" => result!(sessions.swap_records(
            arg!("sessionId", u32),
            &arg!("first", RecordCoordinate),
            &arg!("second", RecordCoordinate),
        )),
        "move_record" => result!(sessions.move_record(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            arg!("targetIndex", usize),
        )),
        "transfer_record" => result!(sessions.transfer_record(
            arg!("sessionId", u32),
            &arg!("coordinate", RecordCoordinate),
            &arg!("destinationFile", String),
            arg!("targetIndex", usize),
        )),
        "list_frontend_plugins" | "list_project_frontend_plugins" => encode(Vec::<Value>::new()),
        "install_frontend_plugin"
        | "uninstall_frontend_plugin"
        | "install_project_frontend_plugin"
        | "uninstall_project_frontend_plugin"
        | "set_project_frontend_plugin_enabled" => Err(EditorError::other(
            "frontend plugin management is not enabled in the Electron preview",
        )),
        _ => Err(EditorError::not_found(format!(
            "unknown editor command `{command}`"
        ))),
    }
}

fn argument<T: serde::de::DeserializeOwned>(
    args: &Map<String, Value>,
    name: &str,
) -> Result<T, EditorError> {
    let value = args
        .get(name)
        .cloned()
        .ok_or_else(|| EditorError::other(format!("missing command argument `{name}`")))?;
    serde_json::from_value(value)
        .map_err(|error| EditorError::other(format!("invalid command argument `{name}`: {error}")))
}

fn optional_argument<T: serde::de::DeserializeOwned>(
    args: &Map<String, Value>,
    name: &str,
) -> Result<Option<T>, EditorError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(|error| {
            EditorError::other(format!("invalid command argument `{name}`: {error}"))
        }),
    }
}

fn encode<T: Serialize>(value: T) -> Result<Value, EditorError> {
    serde_json::to_value(value)
        .map_err(|error| EditorError::other(format!("failed to encode command result: {error}")))
}

fn open_with_default_application(path: &PathBuf) -> Result<(), EditorError> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command.spawn().map(|_| ()).map_err(|error| {
        EditorError::other(format!("failed to open `{}`: {error}", path.display()))
    })
}
