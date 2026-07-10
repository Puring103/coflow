use coflow_cft::CftSchemaView;
use std::collections::BTreeMap;
use std::path::Path;

use super::diagnostics::{table_load_error_diagnostics, TableLoadError};
use super::{TableDiagnostic, TableDiagnostics, TableLocation, TableSheetConfig};

const IMPORT_CONTROL_COLUMN: &str = "#";
const DEFAULT_KEY_COLUMN_ALIASES: &[&str] = &["id", "Id", "ID"];

#[derive(Debug, Clone)]
pub(super) struct ResolvedColumns {
    pub(super) columns: Vec<ResolvedColumn>,
    pub(super) id_column: IdColumn,
    pub(super) control_column: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct IdColumn {
    pub(super) index: usize,
    pub(super) excel_column: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedColumn {
    pub(super) index: usize,
    pub(super) excel_column: usize,
    pub(super) field: String,
    pub(super) field_type: String,
    pub(super) expand: Option<Vec<ExpandedSubColumn>>,
}

#[derive(Debug, Clone)]
pub(super) struct ExpandedSubColumn {
    pub(super) index: usize,
    pub(super) excel_column: usize,
    pub(super) field: String,
    pub(super) field_type: String,
}

pub(super) fn field_columns_from_resolved(
    columns: &[ResolvedColumn],
) -> BTreeMap<Vec<String>, usize> {
    let mut field_columns = BTreeMap::new();
    for column in columns {
        field_columns.insert(vec![column.field.clone()], column.excel_column);
        if let Some(children) = &column.expand {
            for child in children {
                field_columns.insert(
                    vec![column.field.clone(), child.field.clone()],
                    child.excel_column,
                );
            }
        }
    }
    field_columns
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_columns(
    schema: &CftSchemaView,
    source_name: &Path,
    sheet: &TableSheetConfig,
    type_name: &str,
    fields: &BTreeMap<String, String>,
    header_row: &[String],
    header_excel_row: usize,
    header_excel_col: usize,
) -> Result<ResolvedColumns, TableDiagnostics> {
    let mut resolution = ColumnResolution::new(
        schema,
        source_name,
        sheet,
        type_name,
        fields,
        header_row,
        header_excel_row,
        header_excel_col,
    );
    resolution.scan_header();
    resolution.finish()
}

struct ColumnResolution<'a> {
    source_name: &'a Path,
    sheet: &'a TableSheetConfig,
    type_name: &'a str,
    fields: &'a BTreeMap<String, String>,
    header_excel_row: usize,
    header: Vec<(usize, usize, String)>,
    expand_fields: BTreeMap<String, BTreeMap<String, String>>,
    expand_inner_order: BTreeMap<String, Vec<String>>,
    columns: Vec<ResolvedColumn>,
    id_column: Option<IdColumn>,
    control_column: Option<usize>,
    seen_headers: BTreeMap<String, String>,
    seen_fields: BTreeMap<String, String>,
    key_column: String,
    has_explicit_key: bool,
    diagnostics: Vec<TableDiagnostic>,
}

impl<'a> ColumnResolution<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        schema: &CftSchemaView,
        source_name: &'a Path,
        sheet: &'a TableSheetConfig,
        type_name: &'a str,
        fields: &'a BTreeMap<String, String>,
        header_row: &[String],
        header_excel_row: usize,
        header_excel_col: usize,
    ) -> Self {
        let mut header = Vec::with_capacity(header_row.len());
        for (index, cell) in header_row.iter().enumerate() {
            let excel_column = header_excel_col + index;
            let column = table_cell_text(Some(cell));
            header.push((index, excel_column, column.trim().to_string()));
        }

        Self {
            source_name,
            sheet,
            type_name,
            fields,
            header_excel_row,
            header,
            expand_fields: expand_field_index(schema, type_name),
            expand_inner_order: expand_field_order_index(schema, type_name),
            columns: Vec::new(),
            id_column: None,
            control_column: None,
            seen_headers: BTreeMap::new(),
            seen_fields: BTreeMap::new(),
            key_column: sheet.key_column().to_string(),
            has_explicit_key: sheet.key.is_some(),
            diagnostics: Vec::new(),
        }
    }

    fn scan_header(&mut self) {
        let mut cursor = 0;
        while cursor < self.header.len() {
            self.scan_column(&mut cursor);
        }
    }

    fn scan_column(&mut self, cursor: &mut usize) {
        let (index, excel_column, column_text) = &self.header[*cursor];
        let index = *index;
        let excel_column = *excel_column;
        let column_text = column_text.clone();
        *cursor += 1;
        if column_text.is_empty() {
            return;
        }
        if !self.record_header_column(excel_column, &column_text) {
            return;
        }
        if column_text == IMPORT_CONTROL_COLUMN {
            self.control_column = Some(index);
            return;
        }

        let field = self
            .sheet
            .columns
            .get(&column_text)
            .map_or_else(|| column_text.clone(), Clone::clone);
        if is_key_column(
            &column_text,
            &field,
            &self.key_column,
            self.has_explicit_key,
        ) {
            self.record_id_column(index, excel_column, &field, &column_text);
            return;
        }

        let Some(field_type) = self.fields.get(&field).cloned() else {
            self.add_unknown_column_diagnostic(excel_column, column_text, field);
            return;
        };
        if !self.record_field_column(excel_column, &field, &column_text) {
            return;
        }

        let expand = self.expand_fields.get(&field).cloned().map(|child_fields| {
            let inner_order = self
                .expand_inner_order
                .get(&field)
                .cloned()
                .unwrap_or_default();
            self.consume_expanded_columns(
                cursor,
                &ExpandedColumnRequest {
                    parent_index: index,
                    parent_excel_column: excel_column,
                    parent_header: &column_text,
                    parent_field: &field,
                    child_fields: &child_fields,
                    inner_order: &inner_order,
                },
            )
        });

        self.columns.push(ResolvedColumn {
            index,
            excel_column,
            field,
            field_type,
            expand,
        });
    }

    fn record_header_column(&mut self, excel_column: usize, column_text: &str) -> bool {
        if self
            .seen_headers
            .insert(column_text.to_string(), column_text.to_string())
            .is_some()
        {
            self.diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::DuplicateHeaderColumn {
                    location: Box::new(self.location().cell(self.header_excel_row, excel_column)),
                    header: column_text.to_string(),
                },
            ));
            return false;
        }
        true
    }

    fn record_id_column(
        &mut self,
        index: usize,
        excel_column: usize,
        field: &str,
        column_text: &str,
    ) {
        if self.fields.contains_key(field) {
            self.seen_fields
                .insert(field.to_string(), column_text.to_string());
        }
        if self.id_column.is_some() {
            self.diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::DuplicateKeyColumn {
                    location: Box::new(self.location().cell(self.header_excel_row, excel_column)),
                    key: self.key_column.clone(),
                },
            ));
        }
        self.id_column = Some(IdColumn {
            index,
            excel_column,
        });
    }

    fn record_field_column(&mut self, excel_column: usize, field: &str, column_text: &str) -> bool {
        if let Some(first_column) = self
            .seen_fields
            .insert(field.to_string(), column_text.to_string())
        {
            self.diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::DuplicateFieldColumn {
                    location: Box::new(self.location().cell(self.header_excel_row, excel_column)),
                    field: field.to_string(),
                    first_column,
                    duplicate_column: column_text.to_string(),
                },
            ));
            return false;
        }
        true
    }

    fn add_unknown_column_diagnostic(
        &mut self,
        excel_column: usize,
        column_text: String,
        field: String,
    ) {
        self.diagnostics.extend(table_load_error_diagnostics(
            TableLoadError::UnknownColumn {
                location: Box::new(self.location().cell(self.header_excel_row, excel_column)),
                type_name: self.type_name.to_string(),
                column: column_text,
                field,
            },
        ));
    }

    fn consume_expanded_columns(
        &mut self,
        cursor: &mut usize,
        request: &ExpandedColumnRequest<'_>,
    ) -> Vec<ExpandedSubColumn> {
        let mut consumed = Vec::with_capacity(request.inner_order.len());
        if let Some(first_inner) = request.inner_order.first() {
            let inner_ty = request
                .child_fields
                .get(first_inner)
                .cloned()
                .unwrap_or_default();
            consumed.push(ExpandedSubColumn {
                index: request.parent_index,
                excel_column: request.parent_excel_column,
                field: first_inner.clone(),
                field_type: inner_ty,
            });
        }
        for inner_field in request.inner_order.iter().skip(1) {
            if *cursor >= self.header.len() {
                self.diagnostics.extend(table_load_error_diagnostics(
                    TableLoadError::UnknownColumn {
                        location: Box::new(
                            self.location()
                                .cell(self.header_excel_row, request.parent_excel_column),
                        ),
                        type_name: self.type_name.to_string(),
                        column: request.parent_header.to_string(),
                        field: format!(
                            "{} (@expand): not enough columns to cover inner field `{inner_field}`",
                            request.parent_field
                        ),
                    },
                ));
                break;
            }
            let (next_index, next_excel_col, next_text) = &self.header[*cursor];
            if !next_text.is_empty() {
                self.diagnostics.extend(table_load_error_diagnostics(
                    TableLoadError::UnexpectedExpandHeader {
                        location: Box::new(
                            self.location().cell(self.header_excel_row, *next_excel_col),
                        ),
                        parent_field: request.parent_field.to_string(),
                        expected_field: inner_field.clone(),
                        header: next_text.clone(),
                    },
                ));
            }
            let inner_ty = request
                .child_fields
                .get(inner_field)
                .cloned()
                .unwrap_or_default();
            consumed.push(ExpandedSubColumn {
                index: *next_index,
                excel_column: *next_excel_col,
                field: inner_field.clone(),
                field_type: inner_ty,
            });
            *cursor += 1;
        }
        consumed
    }

    fn add_missing_field_diagnostics(&mut self) {
        for field_name in self.fields.keys() {
            if self.seen_fields.contains_key(field_name) {
                continue;
            }
            self.diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::MissingColumn {
                    location: Box::new(self.location().with_row(self.header_excel_row)),
                    type_name: self.type_name.to_string(),
                    field: field_name.clone(),
                },
            ));
        }
    }

    fn finish(mut self) -> Result<ResolvedColumns, TableDiagnostics> {
        self.add_missing_field_diagnostics();
        let Some(id_column) = self.id_column.take() else {
            self.diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::MissingKeyColumn {
                    location: Box::new(self.location().with_row(self.header_excel_row)),
                    type_name: self.type_name.to_string(),
                    key: self.key_column,
                },
            ));
            return Err(TableDiagnostics {
                diagnostics: self.diagnostics,
            });
        };

        if self.diagnostics.is_empty() {
            Ok(ResolvedColumns {
                columns: self.columns,
                id_column,
                control_column: self.control_column,
            })
        } else {
            Err(TableDiagnostics {
                diagnostics: self.diagnostics,
            })
        }
    }

    fn location(&self) -> TableLocation {
        TableLocation::new(self.source_name.to_path_buf()).sheet(self.sheet.sheet.clone())
    }
}

struct ExpandedColumnRequest<'a> {
    parent_index: usize,
    parent_excel_column: usize,
    parent_header: &'a str,
    parent_field: &'a str,
    child_fields: &'a BTreeMap<String, String>,
    inner_order: &'a [String],
}

fn is_key_column(column_text: &str, field: &str, key_column: &str, has_explicit_key: bool) -> bool {
    if has_explicit_key {
        column_text == key_column
    } else {
        DEFAULT_KEY_COLUMN_ALIASES.contains(&column_text)
            || DEFAULT_KEY_COLUMN_ALIASES.contains(&field)
    }
}

fn expand_field_index(
    schema: &CftSchemaView,
    type_name: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let Some(fields) = schema.fields(type_name) else {
        return out;
    };
    for field in fields {
        if !field
            .annotations
            .iter()
            .any(|annotation| annotation.name == "expand")
        {
            continue;
        }
        let Some(inner_fields) = schema.fields(&field.raw_type) else {
            continue;
        };
        let inner_fields = inner_fields
            .map(|inner| (inner.name.clone(), inner.raw_type.clone()))
            .collect();
        out.insert(field.name.clone(), inner_fields);
    }
    out
}

fn expand_field_order_index(
    schema: &CftSchemaView,
    type_name: &str,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(fields) = schema.fields(type_name) else {
        return out;
    };
    for field in fields {
        if !field
            .annotations
            .iter()
            .any(|annotation| annotation.name == "expand")
        {
            continue;
        }
        let Some(inner_fields) = schema.fields(&field.raw_type) else {
            continue;
        };
        let order = inner_fields.map(|inner| inner.name.clone()).collect();
        out.insert(field.name.clone(), order);
    }
    out
}

fn table_cell_text(cell: Option<&String>) -> String {
    cell.cloned().unwrap_or_default()
}
