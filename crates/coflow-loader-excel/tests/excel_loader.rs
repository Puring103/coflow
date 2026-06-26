#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_api::origins_of;
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdErrorCode, CfdValue};
use coflow_loader_excel::{
    collect_input_records, ExcelDiagnostic, ExcelDiagnostics, ExcelSheet, ExcelSource,
};
use coflow_loader_table_core::map_table_diagnostics;
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

fn build_model_from_excel_records(
    schema: &CftContainer,
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
        vec![ExcelSheet::new("物品表")
            .with_type("Item")
            .with_key("物品ID")
            .with_columns([("名称", "name"), ("稀有度", "rarity")])],
    );

    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 2);
    assert!(table.primary_index.contains_key("sword_01"));
    assert!(table.primary_index.contains_key("potion_01"));

    let first_id = table.records[0];
    let Some(first) = model.record(first_id) else {
        return Err("expected first item record".to_string());
    };
    assert_eq!(
        first.field("name"),
        Some(&CfdValue::String("铁剑".to_string()))
    );
    assert_eq!(first.field("id"), None);
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
fn uses_id_header_as_default_record_key() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    )?;
    let path = temp_xlsx_path("default-key");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 1, "name")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "potion")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 1, "Potion")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let table = model
        .table("Item")
        .ok_or_else(|| "expected Item table".to_string())?;

    assert!(table.primary_index.contains_key("potion"));
    Ok(())
}

#[test]
fn reports_missing_sheet_before_read_sheet_errors() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected missing sheet error".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-SHEET")?;
    assert!(diagnostic.message.contains("missing sheet `Missing`"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.file, path);
    assert_eq!(location.sheet.as_deref(), Some("Missing"));
    Ok(())
}

#[test]
fn reports_cell_parse_location_for_bad_cell_values() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected cell parse error".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "CELL-TypeMismatch")?;
    assert!(diagnostic.message.contains("Item.level"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn requires_id_column_on_every_loaded_sheet() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("missing-id-column");
    let mut workbook = Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 0, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 0, 1.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected missing id column error".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-ID")?;
    assert!(diagnostic.message.contains("id"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, None);
    Ok(())
}

#[test]
fn rejects_empty_id_cells_on_data_rows() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("empty-id-cell");
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
        .write_number(1, 1, 1.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected empty id cell error".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-ID")?;
    assert!(diagnostic.message.contains("empty id"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(1));
    Ok(())
}

#[test]
fn rejects_non_identifier_id_cells_on_data_rows() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                level: int;
            }
        "#,
    )?;
    let path = temp_xlsx_path("invalid-id-cell");
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
        .write_string(1, 0, "fire-ball")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 1.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Item")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected invalid id cell error".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-ID")?;
    assert!(diagnostic.message.contains("invalid record key"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(1));
    Ok(())
}

#[test]
fn rejects_excel_error_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-CELL")?;
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(
        diagnostic.message.contains("Error"),
        "expected Error kind, got {}",
        diagnostic.message
    );
    Ok(())
}

#[test]
fn rejects_native_excel_datetime_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-CELL")?;
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(
        diagnostic.message.contains("DateTime"),
        "expected DateTime kind, got {}",
        diagnostic.message
    );
    Ok(())
}

#[test]
fn rejects_typed_iso_excel_datetime_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected unsupported cell value".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-CELL")?;
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.sheet.as_deref(), Some("Item"));
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(2));
    assert!(
        diagnostic.message.contains("DateTimeIso"),
        "expected DateTimeIso kind, got {}",
        diagnostic.message
    );
    Ok(())
}

#[test]
fn accepts_boolean_cells_for_bool_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
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
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 1);
    assert!(table.primary_index.contains_key("item_1"));
    Ok(())
}

#[test]
fn optional_hash_control_column_skips_marked_rows_without_mapping_to_schema() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    let Some(table) = model.table("Item") else {
        return Err("expected Item table".to_string());
    };
    assert_eq!(table.records.len(), 1);
    assert!(!table.primary_index.contains_key("skip_me"));
    assert!(table.primary_index.contains_key("keep_me"));
    Ok(())
}

#[test]
fn rejects_unknown_header_columns_before_model_build() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected unknown column".to_string());
    };

    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-COLUMN")?;
    assert!(diagnostic.message.contains("unknown field `extra`"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn maps_duplicate_id_diagnostics_to_source_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };

    let duplicate = diagnostic_with_code(&err.diagnostics, CfdErrorCode::DuplicateId)?;
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };

    let missing = diagnostic_with_code(&err.diagnostics, CfdErrorCode::MissingRequiredField)?;
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected data model diagnostics".to_string());
    };

    let rows: Vec<usize> = err
        .diagnostics
        .iter()
        .filter(|diag| {
            diag.source
                .as_ref()
                .is_some_and(|source| source.code == CfdErrorCode::MissingRequiredField)
        })
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
                name: string;
                @expand
                env: EnvCfg;
            }
        "#,
    )?;

    let path = temp_xlsx_path("terrain_expand");
    write_terrain_workbook_with_expand(&path).map_err(|err| format!("write workbook: {err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain").with_type("Terrain")]);
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
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
fn expand_rejects_explicit_inner_field_headers() -> TestResult {
    let schema = compile_schema(
        r#"
            @struct sealed type EnvCfg {
                shc: float;
                temperature: float;
                diffusion: float;
            }
            type Terrain {
                @expand
                env: EnvCfg;
            }
        "#,
    )?;

    let path = temp_xlsx_path("terrain-expand-child-headers");
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
        .write_string(0, 2, "temperature")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(0, 3, "diffusion")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "Water")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 4.0)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 2, 20.0)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 3, 0.5)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected explicit @expand child header error".to_string());
    };
    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-COLUMN")?;
    assert!(diagnostic.message.contains("@expand"));
    assert!(diagnostic.message.contains("temperature"));
    assert!(diagnostic.message.contains("empty header"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(3));
    Ok(())
}

#[test]
fn rejects_expand_header_that_would_swallow_normal_column() -> TestResult {
    let schema = compile_schema(
        r#"
            @struct sealed type EnvCfg {
                shc: float;
                temperature: float;
            }
            type Terrain {
                @expand
                env: EnvCfg;
                level: int;
            }
        "#,
    )?;

    let path = temp_xlsx_path("expand-swallowed-normal-column");
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
        .write_string(0, 2, "level")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "Water")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 4.0)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 2, 7.0)
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected @expand header collision error".to_string());
    };
    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-COLUMN")?;
    assert!(diagnostic.message.contains("@expand"));
    assert!(diagnostic.message.contains("temperature"));
    assert!(diagnostic.message.contains("level"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(3));
    Ok(())
}

#[test]
fn resolves_direct_reference_shorthand_cells_by_field_type() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
            type Drop {
                item: Item;
            }
        "#,
    )?;

    let path = temp_xlsx_path("direct-ref-shorthand");
    let mut workbook = Workbook::new();
    let items = workbook
        .add_worksheet()
        .set_name("Item")
        .map_err(|err| format!("{err:?}"))?;
    items
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    items
        .write_string(0, 1, "name")
        .map_err(|err| format!("{err:?}"))?;
    items
        .write_string(1, 0, "sword_01")
        .map_err(|err| format!("{err:?}"))?;
    items
        .write_string(1, 1, "Sword")
        .map_err(|err| format!("{err:?}"))?;

    let drops = workbook
        .add_worksheet()
        .set_name("Drop")
        .map_err(|err| format!("{err:?}"))?;
    drops
        .write_string(0, 0, "id")
        .map_err(|err| format!("{err:?}"))?;
    drops
        .write_string(0, 1, "item")
        .map_err(|err| format!("{err:?}"))?;
    drops
        .write_string(1, 0, "drop_01")
        .map_err(|err| format!("{err:?}"))?;
    drops
        .write_string(1, 1, "&sword_01")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(
        &path,
        vec![
            ExcelSheet::new("Item").with_type("Item"),
            ExcelSheet::new("Drop").with_type("Drop"),
        ],
    );
    let model =
        build_model_from_excel_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let item_id = *model
        .table("Item")
        .and_then(|table| table.primary_index.get("sword_01"))
        .ok_or_else(|| "expected sword_01 item".to_string())?;
    let drop_id = *model
        .table("Drop")
        .and_then(|table| table.primary_index.get("drop_01"))
        .ok_or_else(|| "expected drop_01 drop".to_string())?;
    let drop = model
        .record(drop_id)
        .ok_or_else(|| "expected drop record".to_string())?;

    assert_eq!(
        drop.field("item"),
        Some(&CfdValue::Ref {
            target_type: "Item".to_string(),
            target_key: "sword_01".to_string(),
        })
    );
    let _ = item_id;
    Ok(())
}

#[test]
fn rejects_empty_sheets_and_duplicate_mapped_columns() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
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
    let Err(err) = build_model_from_excel_records(&schema, &[empty_source]) else {
        return Err("expected empty sheet error".to_string());
    };
    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-SHEET")?;
    assert!(diagnostic.message.contains("sheet is empty"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
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
    let Err(err) = build_model_from_excel_records(&schema, &[duplicate_source]) else {
        return Err("expected duplicate mapped column error".to_string());
    };
    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-COLUMN")?;
    assert!(diagnostic.message.contains("key column `id`"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
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
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected @expand header width error".to_string());
    };
    let diagnostic = diagnostic_with_string_code(&err.diagnostics, "EXCEL-COLUMN")?;
    assert!(diagnostic.message.contains("@expand"));
    assert!(diagnostic.message.contains("temperature"));
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.row, Some(1));
    assert_eq!(location.column, Some(2));
    Ok(())
}

#[test]
fn maps_expand_subfield_diagnostics_to_child_columns() -> TestResult {
    let schema = compile_schema(
        r#"
            @struct sealed type EnvCfg {
                shc: float;
                temperature: float;
            }
            type Terrain {
                @expand
                env: EnvCfg;
            }
        "#,
    )?;
    let path = temp_xlsx_path("expand-subfield-origin");
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
        .write_blank(0, 2, &Format::new())
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 0, "Water")
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_number(1, 1, 4.0)
        .map_err(|err| format!("{err:?}"))?;
    sheet
        .write_string(1, 2, "bad-float")
        .map_err(|err| format!("{err:?}"))?;
    workbook.save(&path).map_err(|err| format!("{err:?}"))?;

    let source = ExcelSource::new(&path, vec![ExcelSheet::new("Terrain")]);
    let Err(err) = build_model_from_excel_records(&schema, &[source]) else {
        return Err("expected @expand subfield parse diagnostic".to_string());
    };
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diag| diag.code.starts_with("CELL-"))
        .ok_or_else(|| {
            format!(
                "expected CELL-* diagnostic, got {:?}",
                err.diagnostics
                    .iter()
                    .map(|diag| diag.code.as_str())
                    .collect::<Vec<_>>()
            )
        })?;
    assert!(
        diagnostic.message.contains("temperature"),
        "expected temperature field diagnostic, got {}",
        diagnostic.message
    );
    let location = &diagnostic
        .primary
        .as_ref()
        .ok_or_else(|| "expected primary location".to_string())?
        .location;
    assert_eq!(location.row, Some(2));
    assert_eq!(location.column, Some(3));
    Ok(())
}

fn diagnostic_with_string_code<'a>(
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

fn diagnostic_with_code(
    diagnostics: &[ExcelDiagnostic],
    code: CfdErrorCode,
) -> Result<&ExcelDiagnostic, String> {
    diagnostics
        .iter()
        .find(|diag| {
            diag.source
                .as_ref()
                .is_some_and(|source| source.code == code)
        })
        .ok_or_else(|| {
            format!(
                "expected {code}, got {:?}",
                diagnostics
                    .iter()
                    .filter_map(|diag| diag.source.as_ref().map(|source| source.code))
                    .collect::<Vec<_>>()
            )
        })
}
