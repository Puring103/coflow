//! Shared schema-guided table loading for Coflow data sources.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn)]

mod options;
mod table;
pub mod writer;

pub use options::{TableOptionsError, TableSourceOptions};
pub use table::{
    collect_table_input_records, map_label_to_table, map_label_to_table_origin,
    map_table_diagnostics, resolve_table_write_layout, TableDiagnostic, TableDiagnosticKind,
    TableDiagnostics, TableInputRecords, TableLabel, TableLocation, TableSheet, TableSheetConfig,
    TableSource, TableWriteLayout,
};
