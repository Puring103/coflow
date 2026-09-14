//! Tauri 前端插件的命令、持久化与路径安全边界。
//!
//! 按职责拆分（仅结构拆分，不改行为）：
//! - `manifest`：插件清单校验与 bundle 类型；
//! - `store`：全局/项目作用域持久化与路径安全；
//! - `commands`：7 个 Tauri 命令薄适配。

pub(crate) mod commands;
pub(crate) mod manifest;
pub(crate) mod store;

pub(crate) use commands::{
    install_frontend_plugin, install_project_frontend_plugin, list_frontend_plugins,
    list_project_frontend_plugins, set_project_frontend_plugin_enabled, uninstall_frontend_plugin,
    uninstall_project_frontend_plugin,
};
pub use manifest::{
    FrontendPluginBundle, FrontendPlugins, PluginScope, ProjectFrontendPlugins,
    ProjectPluginDefaults,
};
