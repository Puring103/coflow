#![allow(clippy::panic_in_result_fn)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{
    CfdDataModel, CfdInputValue, CfdValue, RecordOrigin, SourceLocation, TextSpan,
};
use coflow_loader_table_core::{
    collect_table_input_records, map_table_diagnostics, TableSheet, TableSheetConfig, TableSource,
};
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

    let loaded = collect_table_input_records(&schema, &[source])
        .map_err(|err| format!("{err:?}"))?;
    assert_eq!(loaded.records.len(), 1);
    assert!(matches!(
        loaded.records[0].origin,
        RecordOrigin::Table { .. }
    ));
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
fn recognizes_default_id_header_aliases_as_record_key_columns() -> TestResult {
    let schema = compile_schema("type Item { name: string; }")?;

    for header in ["id", "Id", "ID"] {
        let key = format!("item_{header}");
        let source = TableSource::new(
            format!("remote:sht_{header}"),
            vec![TableSheet::new(
                "Item",
                vec![
                    vec![header.to_string(), "name".to_string()],
                    vec![key.clone(), "Potion".to_string()],
                ],
            )],
            vec![TableSheetConfig::new("Item")],
        );

        let loaded = collect_table_input_records(&schema, &[source])
            .map_err(|err| format!("{err:?}"))?;

        assert_eq!(loaded.records.len(), 1, "{header}");
        assert_eq!(loaded.records[0].key, key, "{header}");
    }

    Ok(())
}

#[test]
fn maps_local_table_data_model_diagnostics_to_cells() -> TestResult {
    let schema = compile_schema("type Item { name: string; }")?;
    let source = TableSource::new(
        "data/items.xlsx",
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

    let loaded = collect_table_input_records(&schema, &[source])
        .map_err(|err| format!("{err:?}"))?;
    let origins = loaded
        .records
        .iter()
        .map(|record| record.origin.clone())
        .collect::<Vec<_>>();
    let mut builder = CfdDataModel::builder(&schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let Err(err) = builder.build() else {
        return Err("duplicate table keys should fail".to_string());
    };
    let mapped = map_table_diagnostics(err, &origins);
    let primary = mapped
        .diagnostics
        .first()
        .and_then(|diagnostic| diagnostic.primary.as_ref())
        .ok_or_else(|| "expected mapped primary label".to_string())?;

    assert_eq!(
        primary.location,
        coflow_loader_table_core::TableLocation {
            file: PathBuf::from("data/items.xlsx"),
            sheet: Some("物品表".to_string()),
            row: Some(3),
            column: Some(1),
        }
    );
    Ok(())
}

#[test]
fn maps_file_record_diagnostics_to_record_text_span_through_data_model_location() -> TestResult {
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
    let origins = [RecordOrigin::File {
        path: source_path.clone(),
        span: Some(TextSpan {
            start_line: 4,
            start_character: 2,
            end_line: 6,
            end_character: 1,
        }),
    }];
    let mapped = coflow_data_model::map_diagnostics(err, |id| origins.get(id.index()).cloned());
    let primary = mapped
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

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}
