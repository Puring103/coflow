#![allow(clippy::needless_raw_string_hashes, clippy::panic_in_result_fn)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdErrorCode, CfdIdValue, CfdValue};
use coflow_excel_loader::{
    load_excel, load_excel_model, ExcelDiagnostic, ExcelLoadError, ExcelSheet, ExcelSource,
};
use rust_xlsxwriter::{Workbook, XlsxError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

type TestResult = Result<(), String>;

fn compile_schema(source: &str) -> Result<CftContainer, String> {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .map_err(|err| format!("schema should parse: {err:?}"))?;
    container
        .compile()
        .map_err(|err| format!("schema should compile: {err:?}"))?;
    Ok(container)
}

fn temp_xlsx_path(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("coflow-excel-loader-{name}-{id}.xlsx"))
}

fn write_items_workbook(path: &PathBuf) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("物品表")?;
    sheet.write_string(0, 0, "物品ID")?;
    sheet.write_string(0, 1, "名称")?;
    sheet.write_string(0, 2, "稀有度")?;
    sheet.write_string(0, 3, "tags")?;
    sheet.write_string(1, 0, "sword_01")?;
    sheet.write_string(1, 1, "铁剑")?;
    sheet.write_string(1, 2, "Rare")?;
    sheet.write_string(1, 3, "weapon | melee")?;
    sheet.write_string(3, 0, "potion_01")?;
    sheet.write_string(3, 1, "Potion")?;
    sheet.write_string(3, 2, "Common")?;
    sheet.write_string(3, 3, "consumable")?;
    workbook.save(path)
}

#[test]
fn loads_configured_xlsx_sheets_without_yaml_parsing() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                @id
                id: string;
                name: string;
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
            }
        "#,
    )?;
    let path = temp_xlsx_path("items");
    write_items_workbook(&path).map_err(|err| format!("write workbook: {err:?}"))?;

    let source = ExcelSource::new(
        &path,
        vec![ExcelSheet::new("物品表").with_type("Item").with_columns([
            ("物品ID", "id"),
            ("名称", "name"),
            ("稀有度", "rarity"),
        ])],
    );

    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 2);
    assert!(table
        .primary_index
        .contains_key(&CfdIdValue::from("sword_01")));
    assert!(table
        .primary_index
        .contains_key(&CfdIdValue::from("potion_01")));

    let first_id = table.records[0];
    let Some(first) = model.record(first_id) else {
        return Err("expected first item record".to_string());
    };
    assert_eq!(
        first.field("name"),
        Some(&CfdValue::String("铁剑".to_string()))
    );
    assert_eq!(
        first.field("tags"),
        Some(&CfdValue::Array(vec![
            CfdValue::String("weapon".to_string()),
            CfdValue::String("melee".to_string()),
        ]))
    );
    Ok(())
}

#[test]
fn reports_cell_parse_location_for_bad_cell_values() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("bad-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 1, "not_int")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected cell parse error".to_string());
    };

    let ExcelLoadError::CellParse {
        location, field, ..
    } = err
    else {
        return Err(format!("expected cell parse error, got {err:?}"));
    };
    assert_eq!(field, "level");
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn returns_check_diagnostics_without_discarding_model() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
                level: int;
                check { level > 0; }
            }
        "#,
    )?;
    let path = temp_xlsx_path("check");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, -1.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let output = load_excel(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    let Some(table) = output.model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 1);
    let Some(diagnostics) = output.check_diagnostics else {
        return Err("expected check diagnostics".to_string());
    };
    let diagnostic = diagnostic_with_code(&diagnostics.diagnostics, CfdErrorCode::CheckFailed)?;
    assert_eq!(
        diagnostic
            .primary
            .as_ref()
            .and_then(|label| label.location.row),
        Some(2)
    );
    assert_eq!(
        diagnostic
            .primary
            .as_ref()
            .and_then(|label| label.location.column),
        Some(2)
    );
    Ok(())
}

#[test]
fn rejects_unknown_header_columns_before_model_build() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("unknown-column");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "extra")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unknown column".to_string());
    };

    let ExcelLoadError::UnknownColumn {
        field, location, ..
    } = err
    else {
        return Err(format!("expected unknown column, got {err:?}"));
    };
    assert_eq!(field, "extra");
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn maps_duplicate_id_diagnostics_to_source_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("duplicate-id");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "same")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 1.0)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(2, 0, "same")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(2, 1, 2.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };
    let ExcelLoadError::DataModel(diagnostics) = err else {
        return Err(format!("expected data model diagnostics, got {err:?}"));
    };

    let duplicate = diagnostic_with_code(&diagnostics.diagnostics, CfdErrorCode::DuplicateId)?;
    assert_eq!(
        duplicate
            .primary
            .as_ref()
            .and_then(|label| label.location.row),
        Some(3)
    );
    assert_eq!(
        duplicate
            .primary
            .as_ref()
            .and_then(|label| label.location.column),
        Some(1)
    );
    assert_eq!(
        duplicate
            .related
            .first()
            .map(|label| (label.location.row, label.location.column)),
        Some((Some(2), Some(1)))
    );
    Ok(())
}

#[test]
fn maps_missing_required_field_diagnostics_to_source_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("missing-required");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "missing_level")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };
    let ExcelLoadError::DataModel(diagnostics) = err else {
        return Err(format!("expected data model diagnostics, got {err:?}"));
    };

    let missing =
        diagnostic_with_code(&diagnostics.diagnostics, CfdErrorCode::MissingRequiredField)?;
    assert_eq!(
        missing
            .primary
            .as_ref()
            .and_then(|label| label.location.row),
        Some(2)
    );
    assert_eq!(
        missing
            .primary
            .as_ref()
            .and_then(|label| label.location.column),
        Some(2)
    );
    Ok(())
}

#[test]
fn maps_multiple_invalid_input_rows_to_their_original_excel_rows() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("multiple-invalid");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "missing_level_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(2, 0, "missing_level_2")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };
    let ExcelLoadError::DataModel(diagnostics) = err else {
        return Err(format!("expected data model diagnostics, got {err:?}"));
    };

    let rows: Vec<usize> = diagnostics
        .diagnostics
        .iter()
        .filter(|diag| diag.source.code == CfdErrorCode::MissingRequiredField)
        .filter_map(|diag| diag.primary.as_ref()?.location.row)
        .collect();
    assert_eq!(rows, vec![2, 3]);
    Ok(())
}

fn diagnostic_with_code(
    diagnostics: &[ExcelDiagnostic],
    code: CfdErrorCode,
) -> Result<&ExcelDiagnostic, String> {
    diagnostics
        .iter()
        .find(|diag| diag.source.code == code)
        .ok_or_else(|| {
            format!(
                "expected {code}, got {:?}",
                diagnostics
                    .iter()
                    .map(|diag| diag.source.code)
                    .collect::<Vec<_>>()
            )
        })
}
