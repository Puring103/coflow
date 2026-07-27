//! Versioned, host-neutral extension metadata shared by Coflow editors.
//!
//! Runtime JavaScript APIs remain owned by each editor host. This crate keeps
//! only data that can be validated before an extension is loaded.

use serde::{Deserialize, Serialize};

/// The extension API version implemented by this release.
pub const EXTENSION_API_VERSION: u32 = 1;

/// Metadata stored alongside an installed extension entry module.
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
