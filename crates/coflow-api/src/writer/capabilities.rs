use serde::{Deserialize, Serialize};

/// Static description of a source writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub capabilities: WriterCapabilities,
}

/// Editing capabilities exposed to the front-end so the UI can grey out
/// disabled actions per source.
///
/// The descriptor contains the provider's maximum capability set. Hosts use
/// [`crate::SourceWriter::capabilities`] for the authoritative per-source
/// result when support depends on the resolved storage format.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriterCapabilities {
    pub provider_id: String,
    pub can_edit_field: bool,
    pub can_edit_key: bool,
    pub can_insert_record: bool,
    pub can_delete_record: bool,
    pub requires_full_refresh_after_write: bool,
    pub is_remote: bool,
}

impl WriterCapabilities {
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: false,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            requires_full_refresh_after_write: false,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn local_full() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: true,
            can_delete_record: true,
            requires_full_refresh_after_write: true,
            is_remote: false,
        }
    }

    #[must_use]
    pub fn remote_field_edit() -> Self {
        Self {
            provider_id: String::new(),
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: false,
            can_delete_record: false,
            requires_full_refresh_after_write: true,
            is_remote: true,
        }
    }

    #[must_use]
    pub fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }
}
