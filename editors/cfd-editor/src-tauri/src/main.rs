#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::multiple_crate_versions)]

// 分配器归桌面宿主管理，核心库保留宿主选择权。
#[cfg(windows)]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> tauri::Result<()> {
    cfd_editor_lib::run()
}
