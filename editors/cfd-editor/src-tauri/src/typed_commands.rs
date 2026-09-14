use cfd_editor_core::EditorHost;
#[cfg(feature = "ts-export")]
use specta_typescript::Typescript;
use tauri::{plugin::TauriPlugin, Runtime, State};
use tauri_specta::{collect_commands, Builder as SpectaBuilder};

const PLUGIN_NAME: &str = "typed-editor";

#[tauri::command]
#[specta::specta]
async fn build_project_status(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<bool, String> {
    let host = host.inner().clone();
    super::run_blocking(move || host.sessions().build_project_status(session_id))
        .await
        .map_err(|error| error.message)
}

fn builder<R: Runtime>() -> SpectaBuilder<R> {
    SpectaBuilder::new()
        .plugin_name(PLUGIN_NAME)
        .commands(collect_commands![build_project_status])
}

/// 注册由 Specta 生成前端调用契约的增量类型安全命令。
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = builder();
    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(builder.invoke_handler())
        .build()
}

#[cfg(feature = "ts-export")]
pub fn export_bindings(path: impl AsRef<std::path::Path>) -> Result<(), specta_typescript::Error> {
    builder::<tauri::Wry>().export(Typescript::default(), path)
}
