#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdErrorCode, CfdIdValue, CfdValue};
use coflow_loader_excel::{
    load_excel, load_excel_model, ExcelDiagnostic, ExcelLoadError, ExcelSheet, ExcelSource,
};
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook, XlsxError};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

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
    std::env::temp_dir().join(format!("coflow-loader-excel-{name}-{id}.xlsx"))
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

fn rewrite_xlsx_entry(path: &PathBuf, entry_name: &str, replacement: &str) -> Result<(), String> {
    let input = File::open(path).map_err(|err| format!("open xlsx for rewrite: {err}"))?;
    let mut archive = ZipArchive::new(input).map_err(|err| format!("read xlsx zip: {err}"))?;
    let rewritten_path = path.with_extension("rewritten.xlsx");
    let output =
        File::create(&rewritten_path).map_err(|err| format!("create rewritten xlsx: {err}"))?;
    let mut writer = ZipWriter::new(output);

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| format!("read xlsx entry {index}: {err}"))?;
        let name = file.name().to_string();
        writer
            .start_file(name.clone(), SimpleFileOptions::default())
            .map_err(|err| format!("start xlsx entry {name}: {err}"))?;
        if name == entry_name {
            writer
                .write_all(replacement.as_bytes())
                .map_err(|err| format!("write replacement entry {name}: {err}"))?;
        } else {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|err| format!("read xlsx entry {name}: {err}"))?;
            writer
                .write_all(&bytes)
                .map_err(|err| format!("copy xlsx entry {name}: {err}"))?;
        }
    }

    writer
        .finish()
        .map_err(|err| format!("finish rewritten xlsx: {err}"))?;
    std::fs::rename(&rewritten_path, path).map_err(|err| format!("replace xlsx: {err}"))?;
    Ok(())
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
fn reports_missing_sheet_before_read_sheet_errors() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("missing-sheet");
    let mut workbook = Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Existing")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Missing").with_type("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected missing sheet error".to_string());
    };

    let ExcelLoadError::MissingSheet { file, sheet } = err else {
        return Err(format!("expected missing sheet error, got {err:?}"));
    };
    assert_eq!(file, path);
    assert_eq!(sheet, "Missing");
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
fn rejects_excel_error_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                value: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("error-cell");
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
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let ExcelLoadError::UnsupportedCellValue { location, kind } = err else {
        return Err(format!("expected unsupported cell value, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(kind.contains("Error"), "expected Error kind, got {kind}");
    Ok(())
}

#[test]
fn rejects_native_excel_datetime_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                value: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("datetime-cell");
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
    let datetime = ExcelDateTime::from_ymd(2026, 6, 9).map_err(|err| format!("{err:?}"))?;
    let format = Format::new().set_num_format("yyyy-mm-dd");
    sheet
        .set_column_format(1, &format)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_datetime(1, 1, &datetime)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let ExcelLoadError::UnsupportedCellValue { location, kind } = err else {
        return Err(format!("expected unsupported cell value, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(
        kind.contains("DateTime"),
        "expected DateTime kind, got {kind}"
    );
    Ok(())
}

#[test]
fn rejects_typed_iso_excel_datetime_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                value: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("typed-iso-datetime-cell");
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
        .write_string(1, 1, "placeholder")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    rewrite_xlsx_entry(
        &path,
        "xl/worksheets/sheet1.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <dimension ref="A1:B2"/>
  <sheetData>
    <row r="1">
      <c r="A1" t="inlineStr"><is><t>id</t></is></c>
      <c r="B1" t="inlineStr"><is><t>value</t></is></c>
    </row>
    <row r="2">
      <c r="A2" t="inlineStr"><is><t>item_1</t></is></c>
      <c r="B2" t="d"><v>2026-06-09T00:00:00Z</v></c>
    </row>
  </sheetData>
</worksheet>"#,
    )?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let ExcelLoadError::UnsupportedCellValue { location, kind } = err else {
        return Err(format!("expected unsupported cell value, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(
        kind.contains("DateTimeIso"),
        "expected DateTimeIso kind, got {kind}"
    );
    Ok(())
}

#[test]
fn accepts_boolean_cells_for_bool_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                enabled: bool;
            }
        "#,
    )?;
    let path = temp_xlsx_path("bool-cell");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "enabled")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_boolean(1, 1, true)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    let Some(record_id) = table.records.first().copied() else {
        return Err("expected Item record".to_string());
    };
    let Some(record) = model.record(record_id) else {
        return Err("expected Item record".to_string());
    };
    assert_eq!(record.field("enabled"), Some(&CfdValue::Bool(true)));
    Ok(())
}

#[test]
fn ignores_rows_that_are_empty_in_mapped_columns() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("mapped-empty-row");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(2, 25, "ignored note")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 1);
    assert!(table
        .primary_index
        .contains_key(&CfdIdValue::from("item_1")));
    Ok(())
}

#[test]
fn optional_hash_control_column_skips_marked_rows_without_mapping_to_schema() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("hash-control-column");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "#")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 2, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "##")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 1, "skip_me")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 2, "not_int")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(2, 1, "keep_me")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(2, 2, 7.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 1);
    assert!(!table
        .primary_index
        .contains_key(&CfdIdValue::from("skip_me")));
    assert!(table
        .primary_index
        .contains_key(&CfdIdValue::from("keep_me")));
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

fn write_terrain_workbook_with_expand(path: &PathBuf) -> Result<(), XlsxError> {
    // Sheet shape mirrors a simplified Luban-style layout: the @expand parent
    // header `env` covers columns C..E, and only column C carries the parent
    // header text — the inner-field column slots D..E are header-blank and
    // data-only, exactly as Luban writes its merged-header expansions.
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("Terrain")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "name")?;
    sheet.write_string(0, 2, "env")?; // @expand parent
                                      // D1 / E1 deliberately left blank.
    sheet.write_string(1, 0, "Water")?;
    sheet.write_string(1, 1, "lake")?;
    sheet.write_number(1, 2, 4.0)?; // env.shc
    sheet.write_number(1, 3, 20.0)?; // env.temperature
    sheet.write_number(1, 4, 0.5)?; // env.diffusion
    sheet.write_string(2, 0, "Soil")?;
    sheet.write_string(2, 1, "earth")?;
    sheet.write_number(2, 2, 1.0)?;
    sheet.write_number(2, 3, 25.0)?;
    sheet.write_number(2, 4, 0.1)?;
    workbook.save(path)
}

#[test]
fn expand_consumes_parent_and_adjacent_columns_for_inner_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            enum TerrainKind { Water = 0, Soil = 1, Sand = 2, }
            @struct sealed type EnvCfg {
                shc: float;
                temperature: float;
                diffusion: float;
            }
            type Terrain {
                @id
                id: TerrainKind;
                name: string;
                @expand
                env: EnvCfg;
            }
        "#,
    )?;

    let path = temp_xlsx_path("terrain_expand");
    write_terrain_workbook_with_expand(&path).map_err(|err| format!("write workbook: {err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain").with_type("Terrain")]);
    let model = load_excel_model(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let table = model
        .table("Terrain")
        .ok_or_else(|| "expected Terrain table".to_string())?;
    assert_eq!(table.records.len(), 2);

    let first_id = table.records[0];
    let first = model
        .record(first_id)
        .ok_or_else(|| "expected first record".to_string())?;
    let env = first
        .field("env")
        .ok_or_else(|| "expected env field".to_string())?;
    let CfdValue::Object(env_record) = env else {
        return Err(format!("expected env to be object, got {env:?}"));
    };
    assert_eq!(env_record.field("shc"), Some(&CfdValue::Float(4.0)));
    assert_eq!(
        env_record.field("temperature"),
        Some(&CfdValue::Float(20.0))
    );
    assert_eq!(env_record.field("diffusion"), Some(&CfdValue::Float(0.5)));
    Ok(())
}

#[test]
fn rejects_empty_sheets_and_duplicate_mapped_columns() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                id: string;
                level: int = 0;
            }
        "#,
    )?;

    let empty_path = temp_xlsx_path("empty-sheet");
    Workbook::new()
        .save(&empty_path)
        .map_err(|err| format!("{err:?}"))?;
    let empty_source = ExcelSource::new(
        &empty_path,
        vec![ExcelSheet::new("Sheet1").with_type("Item")],
    );
    let Err(err) = load_excel_model(&schema, &[empty_source]) else {
        return Err("expected empty sheet error".to_string());
    };
    let ExcelLoadError::EmptySheet { location } = err else {
        return Err(format!("expected empty sheet error, got {err:?}"));
    };
    assert_eq!(location.sheet.as_deref(), Some("Sheet1"));

    let duplicate_path = temp_xlsx_path("duplicate-column");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "alias")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "item_1")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 1, "ignored")
        .map_err(|err| format!("{err:?}"))?;
    workbook
        .save(&duplicate_path)
        .map_err(|err| format!("{err:?}"))?;

    let duplicate_source = ExcelSource::new(
        &duplicate_path,
        vec![ExcelSheet::new("Item").with_columns([("alias", "id")])],
    );
    let Err(err) = load_excel_model(&schema, &[duplicate_source]) else {
        return Err("expected duplicate mapped column error".to_string());
    };
    let ExcelLoadError::DuplicateFieldColumn {
        field,
        first_column,
        duplicate_column,
        location,
    } = err
    else {
        return Err(format!("expected duplicate mapped column, got {err:?}"));
    };
    assert_eq!(field, "id");
    assert_eq!(first_column, "id");
    assert_eq!(duplicate_column, "alias");
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn rejects_expand_headers_without_enough_adjacent_columns() -> TestResult {
    let schema = compile_schema(
        r#"
            @struct sealed type EnvCfg {
                shc: float;
                temperature: float;
                diffusion: float;
            }
            type Terrain {
                @id
                id: string;
                @expand
                env: EnvCfg;
            }
        "#,
    )?;
    let path = temp_xlsx_path("expand-too-short");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Terrain")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "env")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "Water")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 4.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain")]);
    let Err(err) = load_excel_model(&schema, &[source]) else {
        return Err("expected @expand header width error".to_string());
    };
    let ExcelLoadError::UnknownColumn {
        field, location, ..
    } = err
    else {
        return Err(format!(
            "expected @expand unknown column error, got {err:?}"
        ));
    };
    assert!(field.contains("@expand"));
    assert!(field.contains("temperature"));
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(2));
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
