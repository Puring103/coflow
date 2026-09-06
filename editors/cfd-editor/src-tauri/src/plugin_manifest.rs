//! 编辑器前端插件的清单元数据。
//!
//! 该类型只用于编辑器宿主内部读写插件包，不是独立的 Rust ABI。

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    pub entry: String,
}
