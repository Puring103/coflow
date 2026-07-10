use coflow_api::{
    CreateTableRequest, DiagnosticSet, SourceLocationSpec, SyncHeaderRequest, TableAddressing,
    TableContext, TableHeaderOptions, TableManager, TableManagerDescriptor, TableOperationResult,
};
use coflow_loader_table_core::TableSheetConfig;
use std::collections::BTreeMap;
use std::fs;

use super::{diag, CsvWriter};
use crate::options::{
    csv_sheet_config_from_options, csv_sheet_for_type_from_options, csv_type_for_sheet_from_options,
};
use crate::{parse, write};

pub static CSV_TABLE_MANAGER_DESCRIPTOR: TableManagerDescriptor = TableManagerDescriptor {
    id: "csv",
    display_name: "CSV table",
    file_extensions: &["csv"],
    aliases: &[],
    addressing: TableAddressing::Sheet,
};

impl TableManager for CsvWriter {
    fn descriptor(&self) -> &'static TableManagerDescriptor {
        &CSV_TABLE_MANAGER_DESCRIPTOR
    }

    fn type_for_sheet(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: Option<&str>,
    ) -> Result<Option<String>, DiagnosticSet> {
        csv_type_for_sheet_from_options(&source.options, sheet)
    }

    fn sheet_for_type(
        &self,
        source: &coflow_api::ResolvedSource,
        actual_type: &str,
    ) -> Result<Option<String>, DiagnosticSet> {
        csv_sheet_for_type_from_options(&source.options, actual_type)
    }

    fn header_options(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: &str,
        actual_type: &str,
    ) -> Result<TableHeaderOptions, DiagnosticSet> {
        Ok(table_header_options(csv_sheet_config_from_options(
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
                "CSV-TABLE",
                "csv table manager requires a local path source",
            )));
        };
        if path.exists() {
            return Err(DiagnosticSet::one(diag(
                "CSV-TABLE",
                format!("file `{}` already exists", path.display()),
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                DiagnosticSet::one(diag(
                    "CSV-TABLE",
                    format!("failed to create `{}`: {err}", parent.display()),
                ))
            })?;
        }
        let rows = vec![request.headers.to_vec()];
        fs::write(path, write(&rows)).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-TABLE",
                format!("failed to write `{}`: {err}", path.display()),
            ))
        })?;
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
                "CSV-TABLE",
                "csv table manager requires a local path source",
            )));
        };
        let text = fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-TABLE",
                format!("failed to read `{}`: {err}", path.display()),
            ))
        })?;
        let mut rows = parse(&text).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-TABLE",
                format!("failed to parse `{}`: {err}", path.display()),
            ))
        })?;
        let old_header = rows.first().cloned().unwrap_or_default();
        let added = added_columns(request.headers, &old_header);
        let removed = removed_columns(request.headers, &old_header);
        rows = sync_rows_to_header(rows, request.headers);
        fs::write(path, write(&rows)).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-TABLE",
                format!("failed to write `{}`: {err}", path.display()),
            ))
        })?;
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

fn sync_rows_to_header(mut rows: Vec<Vec<String>>, new_header: &[String]) -> Vec<Vec<String>> {
    let Some(old_header) = rows.first().cloned() else {
        return vec![new_header.to_vec()];
    };
    let old_index = old_header
        .iter()
        .enumerate()
        .map(|(index, header)| (header.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut out = vec![new_header.to_vec()];
    for row in rows.drain(1..) {
        out.push(
            new_header
                .iter()
                .map(|header| {
                    old_index
                        .get(header)
                        .and_then(|index| row.get(*index))
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
        );
    }
    out
}
