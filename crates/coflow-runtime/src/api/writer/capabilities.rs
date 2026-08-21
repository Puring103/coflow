use serde::{Deserialize, Serialize};

/// Static description of the built-in CFD writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdWriterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub capabilities: WriterCapabilities,
}

/// Editing capabilities exposed to the front-end so the UI can grey out
/// disabled actions.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriterCapabilities {
    pub can_edit_field: bool,
    pub can_edit_key: bool,
    pub can_insert_record: bool,
    pub can_delete_record: bool,
    pub can_reorder_records: bool,
    pub requires_full_refresh_after_write: bool,
}

impl WriterCapabilities {
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            can_edit_field: false,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            can_reorder_records: false,
            requires_full_refresh_after_write: false,
        }
    }

    #[must_use]
    pub fn local_full() -> Self {
        Self {
            can_edit_field: true,
            can_edit_key: true,
            can_insert_record: true,
            can_delete_record: true,
            can_reorder_records: true,
            requires_full_refresh_after_write: true,
        }
    }
}
