use crate::cell_value::CellValueDiagnostics;
use crate::table::{TableDiagnostic, TableDiagnosticKind, TableLocation};
use std::path::PathBuf;

#[derive(Debug)]
pub(super) enum TableLoadError {
    MissingSheet {
        file: PathBuf,
        sheet: String,
    },
    EmptySheet {
        location: Box<TableLocation>,
    },
    UnknownType {
        location: Box<TableLocation>,
        type_name: String,
    },
    UnknownColumn {
        location: Box<TableLocation>,
        type_name: String,
        column: String,
        field: String,
    },
    MissingColumn {
        location: Box<TableLocation>,
        type_name: String,
        field: String,
    },
    DuplicateFieldColumn {
        location: Box<TableLocation>,
        field: String,
        first_column: String,
        duplicate_column: String,
    },
    DuplicateHeaderColumn {
        location: Box<TableLocation>,
        header: String,
    },
    MissingKeyColumn {
        location: Box<TableLocation>,
        type_name: String,
        key: String,
    },
    DuplicateKeyColumn {
        location: Box<TableLocation>,
        key: String,
    },
    UnexpectedExpandHeader {
        location: Box<TableLocation>,
        parent_field: String,
        expected_field: String,
        header: String,
    },
    EmptyIdCell {
        location: Box<TableLocation>,
    },
    InvalidIdCell {
        location: Box<TableLocation>,
        key: String,
        reason: String,
    },
    CellParse {
        location: Box<TableLocation>,
        type_name: String,
        field: String,
        diagnostics: CellValueDiagnostics,
    },
}

#[allow(clippy::too_many_lines)]
pub(super) fn table_load_error_diagnostics(err: TableLoadError) -> Vec<TableDiagnostic> {
    match err {
        TableLoadError::MissingSheet { file, sheet } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::MissingSheet,
            format!(
                "table source `{}` is missing sheet `{sheet}`",
                file.display()
            ),
            TableLocation::new(file).sheet(sheet),
        )],
        TableLoadError::EmptySheet { location } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::EmptySheet,
            "sheet is empty",
            *location,
        )],
        TableLoadError::UnknownType {
            location,
            type_name,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::UnknownType,
            format!("unknown CFT type `{type_name}`"),
            *location,
        )],
        TableLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::UnknownColumn,
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            *location,
        )],
        TableLoadError::MissingColumn {
            location,
            type_name,
            field,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::MissingColumn,
            format!("sheet for type `{type_name}` is missing column for field `{field}`"),
            *location,
        )],
        TableLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::DuplicateFieldColumn,
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            *location,
        )],
        TableLoadError::DuplicateHeaderColumn { location, header } => {
            vec![TableDiagnostic::table_kind(
                TableDiagnosticKind::DuplicateHeaderColumn,
                format!("column header `{header}` appears more than once"),
                *location,
            )]
        }
        TableLoadError::MissingKeyColumn {
            location,
            type_name,
            key,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::MissingKeyColumn,
            format!("sheet for type `{type_name}` must contain key column `{key}`"),
            *location,
        )],
        TableLoadError::DuplicateKeyColumn { location, key } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::DuplicateKeyColumn,
            format!("key column `{key}` is mapped more than once"),
            *location,
        )],
        TableLoadError::UnexpectedExpandHeader {
            location,
            parent_field,
            expected_field,
            header,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::UnexpectedExpandHeader,
            format!(
                "@expand field `{parent_field}` expected adjacent column for `{expected_field}` \
                 to have an empty header, found `{header}`"
            ),
            *location,
        )],
        TableLoadError::EmptyIdCell { location } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::EmptyIdCell,
            "record key cell is empty",
            *location,
        )],
        TableLoadError::InvalidIdCell {
            location,
            key,
            reason,
        } => vec![TableDiagnostic::table_kind(
            TableDiagnosticKind::InvalidIdCell,
            format!("invalid record key `{key}`: {reason}"),
            *location,
        )],
        TableLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| {
                TableDiagnostic::table(
                    format!("CELL-{}", diagnostic.code.as_str()),
                    "CELL",
                    format!("{} while parsing `{type_name}.{field}`", diagnostic.message),
                    (*location).clone(),
                )
                .with_kind(TableDiagnosticKind::CellParse)
            })
            .collect(),
    }
}
