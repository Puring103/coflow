#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes
)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputRecord, CfdInputValue};
use coflow_engine::localization::{format_key, LocalizationKey};

#[test]
fn formats_key_uses_row_id_only() {
    let key = LocalizationKey {
        type_name: "Item".to_string(),
        field_name: "name".to_string(),
        row_id: "potion".to_string(),
    };
    assert_eq!(key.format(), "potion");
    assert_eq!(format_key("Item", "name", "potion"), "potion");
}

fn schema_with_localized_string() -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(
            ModuleId::from("schema/main.cft"),
            r#"
            type Item {
              @localized
              name: string;
            }
            "#,
        )
        .expect("schema source compiles");
    container.compile().expect("compile succeeds");
    container
}

fn build_simple_model() -> (CftContainer, CfdDataModel) {
    let schema = schema_with_localized_string();
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [("name", CfdInputValue::from("Potion"))],
    ));
    let model = builder.build().expect("model builds");
    (schema, model)
}

#[test]
fn schema_publishes_localized_field_metadata() {
    let (schema, _) = build_simple_model();
    let item = schema.resolve_type("Item").unwrap();
    let field = item.all_fields.iter().find(|f| f.name == "name").unwrap();
    assert!(field.is_localized);
}

#[test]
fn singleton_schema_publishes_is_singleton() {
    let mut container = CftContainer::new();
    container
        .add_module(
            ModuleId::from("schema/main.cft"),
            "@singleton type Cfg { value: int; }",
        )
        .expect("source compiles");
    container.compile().expect("compile succeeds");
    let cfg = container.resolve_type("Cfg").unwrap();
    assert!(cfg.is_singleton);
}
