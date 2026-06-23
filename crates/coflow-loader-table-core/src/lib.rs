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

pub mod cell_value;
mod table;

pub use table::{
    collect_table_input_records, map_label_to_table, map_table_diagnostics, TableDiagnostic,
    TableDiagnostics, TableInputRecords, TableLabel, TableLocation, TableSheet, TableSheetConfig,
    TableSource,
};
