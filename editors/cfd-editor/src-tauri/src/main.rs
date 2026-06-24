#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::multiple_crate_versions)]

fn main() -> tauri::Result<()> {
    cfd_editor_lib::run()
}
