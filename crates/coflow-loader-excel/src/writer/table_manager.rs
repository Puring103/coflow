use calamine::Reader;
use coflow_api::{
    CreateTableRequest, DiagnosticSet, SourceLocationSpec, SyncHeaderRequest, TableAddressing,
    TableContext, TableHeaderOptions, TableManager, TableManagerDescriptor, TableOperationResult,
};
use coflow_loader_table_core::TableSheetConfig;
use std::collections::BTreeMap;
use std::path::Path;

use super::{diag, excel_cell_to_text, ExcelWriter};
use crate::options::{
    excel_sheet_config_from_options, excel_sheet_for_type_from_options,
    excel_type_for_sheet_from_options,
};

pub static EXCEL_TABLE_MANAGER_DESCRIPTOR: TableManagerDescriptor = TableManagerDescriptor {
    id: "excel",
    display_name: "Excel table",
    file_extensions: &["xlsx", "xlsm", "xls"],
    aliases: &["xlsx"],
    addressing: TableAddressing::Sheet,
};

impl TableManager for ExcelWriter {
    fn descriptor(&self) -> &'static TableManagerDescriptor {
        &EXCEL_TABLE_MANAGER_DESCRIPTOR
    }

    fn type_for_sheet(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: Option<&str>,
    ) -> Result<Option<String>, DiagnosticSet> {
        excel_type_for_sheet_from_options(&source.options, sheet)
    }

    fn sheet_for_type(
        &self,
        source: &coflow_api::ResolvedSource,
        actual_type: &str,
    ) -> Result<Option<String>, DiagnosticSet> {
        excel_sheet_for_type_from_options(&source.options, actual_type)
    }

    fn header_options(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: &str,
        actual_type: &str,
    ) -> Result<TableHeaderOptions, DiagnosticSet> {
        Ok(table_header_options(excel_sheet_config_from_options(
            &source.options,
            sheet,
            actual_type,
        )?))
    }

    fn create_table(
        &self,
        _ctx: TableContext<'_>,
        request: &CreateTableRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "EXCEL-TABLE",
                "excel table manager requires a local path source",
            )));
        };
        if path.exists() {
            append_excel_sheet(path, request.sheet, request.headers)?;
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    DiagnosticSet::one(diag(
                        "EXCEL-TABLE",
                        format!("failed to create `{}`: {err}", parent.display()),
                    ))
                })?;
            }
            create_excel_file(path, request.sheet, request.headers)?;
        }
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added: Vec::new(),
            removed: Vec::new(),
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn sync_header(
        &self,
        _ctx: TableContext<'_>,
        request: &SyncHeaderRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "EXCEL-TABLE",
                "excel table manager requires a local path source",
            )));
        };
        let sheet = request.sheet.unwrap_or(request.actual_type);
        let mut created_sheet = false;
        let old_header = excel_header(path, sheet).or_else(|diagnostics| {
            if excel_sheet_missing(&diagnostics) {
                append_excel_sheet(path, sheet, request.headers)?;
                created_sheet = true;
                Ok(Vec::new())
            } else {
                Err(diagnostics)
            }
        })?;
        let added = added_columns(request.headers, &old_header);
        let removed = removed_columns(request.headers, &old_header);
        if !created_sheet {
            sync_excel_header(path, sheet, request.headers)?;
        }
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added,
            removed,
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn table_header_options(config: TableSheetConfig) -> TableHeaderOptions {
    let mut out = TableHeaderOptions::new(config.sheet);
    if let Some(type_name) = config.type_name {
        out = out.with_type(type_name);
    }
    if let Some(key) = config.key {
        out = out.with_key(key);
    }
    out.with_columns(config.columns)
}

fn create_excel_file(path: &Path, sheet: &str, headers: &[String]) -> Result<(), DiagnosticSet> {
    let mut book = umya_spreadsheet::new_file();
    if sheet != "Sheet1" {
        let existing = book.get_sheet_by_name_mut("Sheet1").ok_or_else(|| {
            DiagnosticSet::one(diag("EXCEL-TABLE", "default worksheet is missing"))
        })?;
        existing.set_name(sheet);
    }
    write_excel_headers(&mut book, sheet, headers)?;
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to write `{}`: {err:?}", path.display()),
        ))
    })
}

fn append_excel_sheet(path: &Path, sheet: &str, headers: &[String]) -> Result<(), DiagnosticSet> {
    let mut book = umya_spreadsheet::reader::xlsx::read(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to read `{}`: {err:?}", path.display()),
        ))
    })?;
    if book.get_sheet_by_name(sheet).is_some() {
        return Err(DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("sheet `{sheet}` already exists in `{}`", path.display()),
        )));
    }
    book.new_sheet(sheet).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!(
                "failed to create sheet `{sheet}` in `{}`: {err}",
                path.display()
            ),
        ))
    })?;
    write_excel_headers(&mut book, sheet, headers)?;
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to write `{}`: {err:?}", path.display()),
        ))
    })
}

fn write_excel_headers(
    book: &mut umya_spreadsheet::Spreadsheet,
    sheet: &str,
    headers: &[String],
) -> Result<(), DiagnosticSet> {
    let worksheet = book.get_sheet_by_name_mut(sheet).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("sheet `{sheet}` not found after workbook update"),
        ))
    })?;
    for (index, header) in headers.iter().enumerate() {
        let column = u32::try_from(index + 1)
            .map_err(|_| DiagnosticSet::one(diag("EXCEL-TABLE", "too many columns for Excel")))?;
        worksheet.get_cell_mut((column, 1_u32)).set_value(header);
    }
    Ok(())
}

fn excel_header(path: &Path, sheet: &str) -> Result<Vec<String>, DiagnosticSet> {
    let mut workbook = calamine::open_workbook_auto(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to read `{}`: {err}", path.display()),
        ))
    })?;
    let range = workbook.worksheet_range(sheet).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("sheet `{sheet}` not found in `{}`: {err}", path.display()),
        ))
    })?;
    Ok(range
        .rows()
        .next()
        .map(|row| row.iter().map(excel_cell_to_text).collect())
        .unwrap_or_default())
}

fn excel_sheet_missing(diagnostics: &DiagnosticSet) -> bool {
    diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "EXCEL-TABLE"
            && diagnostic.message.contains("sheet `")
            && diagnostic.message.contains("not found")
    })
}

fn sync_excel_header(
    path: &Path,
    sheet_name: &str,
    new_header: &[String],
) -> Result<(), DiagnosticSet> {
    let old_header = excel_header(path, sheet_name)?;
    let mut old_index = BTreeMap::new();
    for (index, header) in old_header.iter().enumerate() {
        let column = u32::try_from(index + 1)
            .map_err(|_| DiagnosticSet::one(diag("EXCEL-TABLE", "too many columns for Excel")))?;
        old_index.insert(header.clone(), column);
    }
    let mut book = umya_spreadsheet::reader::xlsx::read(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to read `{}`: {err:?}", path.display()),
        ))
    })?;
    let sheet = book.get_sheet_by_name_mut(sheet_name).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("sheet `{sheet_name}` not found in `{}`", path.display()),
        ))
    })?;
    let (_max_column, max_row) = sheet.get_highest_column_and_row();
    let mut rows = Vec::new();
    for row in 2..=max_row {
        let values = new_header
            .iter()
            .map(|header| {
                old_index
                    .get(header)
                    .and_then(|column| sheet.get_cell((*column, row)))
                    .map_or_else(String::new, |cell| cell.get_value().to_string())
            })
            .collect::<Vec<_>>();
        rows.push(values);
    }
    if !old_header.is_empty() {
        let count = u32::try_from(old_header.len())
            .map_err(|_| DiagnosticSet::one(diag("EXCEL-TABLE", "too many columns for Excel")))?;
        sheet.remove_column_by_index(&1, &count);
    }
    for (index, header) in new_header.iter().enumerate() {
        let column = u32::try_from(index + 1)
            .map_err(|_| DiagnosticSet::one(diag("EXCEL-TABLE", "too many columns for Excel")))?;
        sheet.get_cell_mut((column, 1_u32)).set_value(header);
    }
    for (row_index, row) in rows.iter().enumerate() {
        let excel_row = u32::try_from(row_index + 2)
            .map_err(|_| DiagnosticSet::one(diag("EXCEL-TABLE", "too many rows for Excel")))?;
        for (column_index, value) in row.iter().enumerate() {
            let excel_column = u32::try_from(column_index + 1).map_err(|_| {
                DiagnosticSet::one(diag("EXCEL-TABLE", "too many columns for Excel"))
            })?;
            sheet
                .get_cell_mut((excel_column, excel_row))
                .set_value(value);
        }
    }
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-TABLE",
            format!("failed to write `{}`: {err:?}", path.display()),
        ))
    })
}

fn added_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let old = old_header.iter().collect::<std::collections::BTreeSet<_>>();
    new_header
        .iter()
        .filter(|header| !old.contains(header))
        .cloned()
        .collect()
}

fn removed_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let new = new_header.iter().collect::<std::collections::BTreeSet<_>>();
    old_header
        .iter()
        .filter(|header| !new.contains(header))
        .cloned()
        .collect()
}
