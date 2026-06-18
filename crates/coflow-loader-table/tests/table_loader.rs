#![allow(clippy::panic_in_result_fn)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::CfdValue;
use coflow_loader_table::{
    collect_table_input_records, load_table_model, TableSheet, TableSheetConfig, TableSource,
};

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

    let model = load_table_model(
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
