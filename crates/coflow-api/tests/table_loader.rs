#![allow(clippy::panic_in_result_fn)]

use coflow_api::table::{collect_table_input_records, TableSheet, TableSheetConfig, TableSource};
use coflow_api::{OriginMap, SourceLocation, TextSpan};
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputValue, CfdValue};
use std::path::PathBuf;

type TestResult = Result<(), String>;

#[test]
fn loads_table_source_with_excel_style_sheet_config() -> TestResult {
    let schema = compile_schema(
        r"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                name: string;
                rarity: Rarity = Rarity.Common;
            }
        ",
    )?;
    let source = TableSource::new(
        "remote:sht_test",
        vec![TableSheet::new(
            "物品表",
            vec![
                vec![
                    "物品ID".to_string(),
                    "名称".to_string(),
                    "稀有度".to_string(),
                ],
                vec![
                    "sword_01".to_string(),
                    "铁剑".to_string(),
                    "Rare".to_string(),
                ],
            ],
        )],
        vec![TableSheetConfig::new("物品表")
            .with_type("Item")
            .with_key("物品ID")
            .with_columns([("名称", "name"), ("稀有度", "rarity")])],
    );

    let loaded =
        collect_table_input_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.origins.record_count(), 1);
    assert_eq!(loaded.records[0].key, "sword_01");

    let loaded = collect_table_input_records(
        &schema,
        &[TableSource::new(
            "remote:sht_test",
            vec![TableSheet::new(
                "物品表",
                vec![
                    vec![
                        "物品ID".to_string(),
                        "名称".to_string(),
                        "稀有度".to_string(),
                    ],
                    vec![
                        "sword_01".to_string(),
                        "铁剑".to_string(),
                        "Rare".to_string(),
                    ],
                ],
            )],
            vec![TableSheetConfig::new("物品表")
                .with_type("Item")
                .with_key("物品ID")
                .with_columns([("名称", "name"), ("稀有度", "rarity")])],
        )],
    )
    .map_err(|err| format!("{err:?}"))?;
    let mut builder = CfdDataModel::builder(&schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let model = builder
        .build()
        .map_err(|err| format!("data model should build: {err:?}"))?;
    let item = model
        .table("Item")
        .and_then(|table| table.primary_index.get("sword_01"))
        .and_then(|record_id| model.record(*record_id))
        .ok_or_else(|| "expected sword_01 item".to_string())?;
    assert_eq!(
        item.field("name"),
        Some(&CfdValue::String("铁剑".to_string()))
    );
    Ok(())
}

#[test]
fn maps_remote_table_data_model_diagnostics_to_remote_cells() -> TestResult {
    let schema = compile_schema("type Item { name: string; }")?;
    let source = TableSource::remote(
        "lark:sht_test",
        "https://example.feishu.cn/wiki/wiki_token",
        vec![TableSheet::new(
            "物品表",
            vec![
                vec!["物品ID".to_string(), "名称".to_string()],
                vec!["sword_01".to_string(), "铁剑".to_string()],
                vec!["sword_01".to_string(), "短剑".to_string()],
            ],
        )],
        vec![TableSheetConfig::new("物品表")
            .with_type("Item")
            .with_key("物品ID")
            .with_columns([("名称", "name")])],
    );

    let loaded =
        collect_table_input_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;
    let mut builder = CfdDataModel::builder(&schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let Err(err) = builder.build() else {
        return Err("duplicate table keys should fail".to_string());
    };
    let mapped = loaded.origins.to_origin_map().map_diagnostics(err);
    let primary = mapped
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.primary.as_ref())
        .ok_or_else(|| "expected mapped primary label".to_string())?;

    assert_eq!(
        primary.location,
        SourceLocation::RemoteCell {
            document: "https://example.feishu.cn/wiki/wiki_token".to_string(),
            sheet: Some("物品表".to_string()),
            row: 3,
            column: 1,
        }
    );
    Ok(())
}

#[test]
fn maps_file_record_diagnostics_to_record_text_span() -> TestResult {
    let schema = compile_schema("type Item { value: int; }")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let Err(err) = builder.build() else {
        return Err("missing value should fail".to_string());
    };

    let source_path = PathBuf::from("data/items.cfd");
    let mut origins = OriginMap::default();
    origins.push_file_record(
        source_path.clone(),
        Some(TextSpan {
            start_line: 4,
            start_character: 2,
            end_line: 6,
            end_character: 1,
        }),
    );
    let mapped = origins.map_diagnostics(err);
    let primary = mapped
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.primary.as_ref())
        .ok_or_else(|| "expected mapped primary label".to_string())?;

    assert_eq!(
        primary.location,
        SourceLocation::FileSpan {
            path: source_path,
            start_line: 4,
            start_character: 2,
            end_line: 6,
            end_character: 1,
        }
    );
    Ok(())
}

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
