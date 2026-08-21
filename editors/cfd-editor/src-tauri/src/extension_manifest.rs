//! Manifest metadata used while installing a frontend editor plugin.
//!
//! This is intentionally an editor-internal DTO. It is not a versioned Rust
//! ABI and does not belong in a standalone workspace crate.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    pub entry: String,
}
