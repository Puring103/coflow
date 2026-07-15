#![allow(clippy::panic_in_result_fn)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::RecordOrigin;
use coflow_loader_table_core::{
    collect_table_input_records, TableSheet, TableSheetConfig, TableSource,
};

type TestResult = Result<(), String>;

#[test]
fn collects_table_rows_as_input_records() -> TestResult {
    let schema = compile_schema("type Item { name: string; }")?;
    let source = TableSource::new(
        "items.xlsx",
        vec![TableSheet::new(
            "Item",
            vec![
                vec!["id".to_string(), "name".to_string()],
                vec!["sword".to_string(), "Sword".to_string()],
            ],
        )],
        vec![TableSheetConfig::new("Item")],
    );

    let loaded =
        collect_table_input_records(&schema, &[source]).map_err(|err| format!("{err:?}"))?;

    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.records[0].key, "sword");
    assert!(matches!(
        loaded.records[0].origin,
        RecordOrigin::Table { .. }
    ));
    Ok(())
}

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}
