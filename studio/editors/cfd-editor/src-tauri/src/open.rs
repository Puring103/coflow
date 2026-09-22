//! 系统默认应用打开文件。平台分支收敛于此，命令层只调用它。

use std::path::Path;

use cfd_editor_core::editor::EditorError;

/// 用系统默认应用打开 `path`，与具体编辑器命令无关。
pub(crate) fn open_path(path: &Path) -> Result<(), EditorError> {
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
