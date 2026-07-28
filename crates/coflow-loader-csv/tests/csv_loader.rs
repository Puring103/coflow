#![allow(clippy::panic_in_result_fn)]

use coflow_api::origins_of;
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{CfdDataModel, CfdValue};
use coflow_loader_csv::{collect_input_records, CsvSheet, CsvSource};
use coflow_loader_table_core::map_table_diagnostics;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

type TestResult = Result<(), String>;

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}

fn build_model(schema: &CftSchema, sources: &[CsvSource]) -> Result<CfdDataModel, String> {
    let loaded = collect_input_records(schema, sources).map_err(|err| format!("{err:?}"))?;
    let origins = origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_loaded_record(record);
    }
    builder
        .build()
        .map_err(|err| format!("{:?}", map_table_diagnostics(err, &origins)))
}

fn temp_csv_path(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("coflow-loader-csv-{name}-{id}.csv"))
}

fn temp_named_csv_path(name: &str) -> Result<PathBuf, String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("coflow-loader-csv-named-{id}"));
    std::fs::create_dir_all(&dir).map_err(|err| format!("create temp dir: {err}"))?;
    Ok(dir.join(format!("{name}.csv")))
}

#[test]
fn loads_configured_csv_as_table_source() -> TestResult {
    let schema = compile_schema(
        r"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                name: string;
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
            }
        ",
    )?;
    let path = temp_csv_path("items");
    std::fs::write(
        &path,
        "物品ID,名称,稀有度,tags\nsword_01,铁剑,Rare,weapon | melee\npotion_01,Potion,Common,consumable\n",
    )
    .map_err(|err| format!("write csv: {err}"))?;

    let source = CsvSource::new(
        &path,
        vec![CsvSheet::new("物品表")
            .with_type("Item")
            .with_key("物品ID")
            .with_columns([("名称", "name"), ("稀有度", "rarity")])],
    );

    let model = build_model(&schema, &[source])?;
    let table = model
        .table("Item")
        .ok_or_else(|| "expected Item table".to_string())?;
    assert_eq!(table.records.len(), 2);
    let first = model
        .record(table.records[0])
        .ok_or_else(|| "expected first record".to_string())?;
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
fn defaults_sheet_and_type_to_csv_file_stem() -> TestResult {
    let schema = compile_schema("type Item { name: string; }")?;
    let path = temp_named_csv_path("Item")?;
    std::fs::write(&path, "id,name\npotion,Potion\n").map_err(|err| format!("{err}"))?;

    let model = build_model(&schema, &[CsvSource::new(&path, Vec::new())])?;
    let table = model
        .table("Item")
        .ok_or_else(|| "expected Item table".to_string())?;

    assert!(table.primary_index.contains_key("potion"));
    Ok(())
}

#[test]
fn parses_and_writes_rfc4180_csv_for_shared_callers() -> TestResult {
    let source = "id,text\nitem_1,\"hello, \"\"world\"\"\"\nitem_2,\"line 1\nline 2\"\n";

    let rows = coflow_loader_csv::parse(source)?;

    assert_eq!(rows[1], vec!["item_1", "hello, \"world\""]);
    assert_eq!(coflow_loader_csv::write(&rows), source);
    Ok(())
}
