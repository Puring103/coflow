#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_api::origins_of;
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::CfdDataModel;
use coflow_loader_excel::{
    collect_input_records, ExcelDiagnostic, ExcelDiagnostics, ExcelSheet, ExcelSource,
};
use coflow_loader_table_core::map_table_diagnostics;
use rust_xlsxwriter::{Formula, Workbook};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

type TestResult = Result<(), String>;

fn build_model_from_excel_records(
    schema: &CftSchema,
    sources: &[ExcelSource],
) -> Result<CfdDataModel, ExcelDiagnostics> {
    let loaded = collect_input_records(schema, sources)?;
    let origins = origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    builder
        .build()
        .map_err(|diagnostics| ExcelDiagnostics::from(map_table_diagnostics(diagnostics, &origins)))
}

#[test]
fn excel_error_codes_have_negative_location_and_adjacent_valid_coverage() -> TestResult {
    let cases = [
        error_open_workbook()?,
        error_missing_sheet()?,
        error_unknown_type()?,
        error_unknown_column()?,
        error_missing_id()?,
        error_unsupported_cell()?,
    ];
    for case in cases {
        assert_eq!(case.diagnostic.code, case.code, "{}", case.name);
        assert_eq!(case.diagnostic.stage, "EXCEL", "{}", case.name);
        let primary = case
            .diagnostic
            .primary
            .as_ref()
            .ok_or_else(|| format!("{} missing primary label", case.name))?;
        assert_eq!(primary.location.row, case.row, "{}", case.name);
        assert_eq!(primary.location.column, case.column, "{}", case.name);
    }

    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("valid-adjacent");
    write_item_workbook(&path, "Item", "value", "item_1", 1.0)?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    Ok(())
}

struct ErrorCase {
    name: &'static str,
    code: &'static str,
    row: Option<usize>,
    column: Option<usize>,
    diagnostic: ExcelDiagnostic,
}

fn error_open_workbook() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("missing-workbook");
    let err = build_model_from_excel_records(
        &schema,
        &[ExcelSource::new(&path, vec![ExcelSheet::new("Item")])],
    )
    .expect_err("missing workbook should fail");
    Ok(ErrorCase {
        name: "open workbook",
        code: "EXCEL-OPEN",
        row: None,
        column: None,
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-OPEN")?.clone(),
    })
}

fn error_missing_sheet() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("missing-sheet");
    write_item_workbook(&path, "Other", "value", "item_1", 1.0)?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let err =
        build_model_from_excel_records(&schema, &[source]).expect_err("missing sheet should fail");
    Ok(ErrorCase {
        name: "missing sheet",
        code: "EXCEL-SHEET",
        row: None,
        column: None,
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-SHEET")?.clone(),
    })
}

fn error_unknown_type() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("unknown-type");
    write_item_workbook(&path, "Item", "value", "item_1", 1.0)?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item").with_type("Missing")]);
    let err =
        build_model_from_excel_records(&schema, &[source]).expect_err("unknown type should fail");
    Ok(ErrorCase {
        name: "unknown type",
        code: "EXCEL-TYPE",
        row: None,
        column: None,
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-TYPE")?.clone(),
    })
}

fn error_unknown_column() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("unknown-column");
    write_item_workbook(&path, "Item", "missing", "item_1", 1.0)?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let err =
        build_model_from_excel_records(&schema, &[source]).expect_err("unknown column should fail");
    Ok(ErrorCase {
        name: "unknown column",
        code: "EXCEL-COLUMN",
        row: Some(1),
        column: Some(2),
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-COLUMN")?.clone(),
    })
}

fn error_missing_id() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: int; }\n")?;
    let path = temp_xlsx_path("missing-id");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "value")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let err =
        build_model_from_excel_records(&schema, &[source]).expect_err("missing id should fail");
    Ok(ErrorCase {
        name: "missing id",
        code: "EXCEL-ID",
        row: Some(1),
        column: None,
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-ID")?.clone(),
    })
}

fn error_unsupported_cell() -> Result<ErrorCase, String> {
    let schema = compile_schema("type Item { value: bool; }\n")?;
    let path = temp_xlsx_path("unsupported-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "value")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_formula(1, 1, Formula::new("=1/0").set_result("#DIV/0!"))
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;
    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let err = build_model_from_excel_records(&schema, &[source])
        .expect_err("unsupported cell should fail");
    Ok(ErrorCase {
        name: "unsupported cell",
        code: "EXCEL-CELL",
        row: Some(2),
        column: Some(2),
        diagnostic: diagnostic_with_code(&err.diagnostics, "EXCEL-CELL")?.clone(),
    })
}

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}

fn write_item_workbook(
    path: &PathBuf,
    sheet_name: &str,
    value_header: &str,
    id: &str,
    value: f64,
) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name(sheet_name)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, value_header)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, id)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, value)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(path).map_err(|err| format!("{err:?}"))
}

fn diagnostic_with_code<'a>(
    diagnostics: &'a [ExcelDiagnostic],
    code: &str,
) -> Result<&'a ExcelDiagnostic, String> {
    diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .ok_or_else(|| {
            format!(
                "expected {code}, got {:?}",
                diagnostics
                    .iter()
                    .map(|diag| diag.code.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

fn temp_xlsx_path(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "coflow-loader-excel-error-coverage-{name}-{id}.xlsx"
    ))
}
