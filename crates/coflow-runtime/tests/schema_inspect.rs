#![allow(clippy::expect_used, clippy::panic)]

use coflow_project::Project;
use coflow_runtime::{inspect_schema, schema_files, Runtime};

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r"
            @idAsEnum(ItemId)
            type Item {
                name: string;
                rarity: Rarity = Rarity.Common;
            }
            enum Rarity {
                Common = 0,
                Rare = 10,
            }

            enum ItemId {}
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_large_i64_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r"
            const HUGE: int = 9007199254740993;

            type Holder {
                count: int = 9007199254740993;
                rarity: Rarity = Rarity.Huge;
            }

            enum Rarity {
                Huge = 9007199254740993,
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_ref_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r"
            type Item {
                name: string;
            }

            type Holder {
                item: &Item;
                backup: &Item? = null;
                items: [&Item];
                by_name: {string: &Item};
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

#[test]
fn inspect_schema_preserves_id_as_enum_fields_and_enums() {
    let root = std::env::temp_dir().join(format!("coflow-schema-inspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = Runtime::open_schema_session(project).expect("schema session");
    let report = inspect_schema(&session, None, false);

    let item = report
        .types
        .iter()
        .find(|ty| ty.name == "Item")
        .expect("Item type");
    assert!(item.annotations.iter().any(|a| a.name == "idAsEnum"));
    assert!(item.fields.iter().any(|field| field.name == "name"));
    assert!(report
        .enums
        .iter()
        .any(|e| e.name == "Rarity" && e.variants.iter().any(|v| v.name == "Common")));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn inspect_schema_serializes_large_i64_values_as_strings() {
    let root = std::env::temp_dir().join(format!("coflow-schema-i64-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_large_i64_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = Runtime::open_schema_session(project).expect("schema session");
    let json = serde_json::to_value(inspect_schema(&session, None, false)).expect("json value");

    assert_eq!(json["consts"][0]["value"]["value"], "9007199254740993");
    assert_eq!(json["enums"][0]["variants"][0]["value"], "9007199254740993");

    let holder = json["types"]
        .as_array()
        .expect("types array")
        .iter()
        .find(|ty| ty["name"] == "Holder")
        .expect("Holder type");
    let fields = holder["fields"].as_array().expect("fields array");
    let count = fields
        .iter()
        .find(|field| field["name"] == "count")
        .expect("count field");
    assert_eq!(count["default"]["value"], "9007199254740993");

    let rarity = fields
        .iter()
        .find(|field| field["name"] == "rarity")
        .expect("rarity field");
    assert_eq!(rarity["default"]["value"]["value"], "9007199254740993");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn inspect_schema_serializes_ref_type_shapes() {
    let root =
        std::env::temp_dir().join(format!("coflow-schema-inspect-ref-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_ref_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = Runtime::open_schema_session(project).expect("schema session");
    let json =
        serde_json::to_value(inspect_schema(&session, Some("Holder"), false)).expect("json value");

    let holder = json["types"][0].as_object().expect("Holder type object");
    let fields = holder["fields"].as_array().expect("fields array");
    let field_ty = |name: &str| {
        fields
            .iter()
            .find(|field| field["name"] == name)
            .unwrap_or_else(|| panic!("missing field {name}"))["ty"]
            .clone()
    };
    assert_eq!(
        field_ty("item"),
        serde_json::json!({ "kind": "ref", "target": "Item" })
    );
    assert_eq!(
        field_ty("backup"),
        serde_json::json!({
            "kind": "nullable",
            "inner": { "kind": "ref", "target": "Item" },
        })
    );
    assert_eq!(
        field_ty("items"),
        serde_json::json!({
            "kind": "array",
            "item": { "kind": "ref", "target": "Item" },
        })
    );
    assert_eq!(
        field_ty("by_name"),
        serde_json::json!({
            "kind": "dict",
            "key": { "kind": "string" },
            "value": { "kind": "ref", "target": "Item" },
        })
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn schema_files_returns_compiled_module_sources() {
    let root = std::env::temp_dir().join(format!("coflow-schema-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = Runtime::open_schema_session(project).expect("schema session");
    let files = schema_files(&session);

    assert_eq!(files.files.len(), 1);
    assert!(files.files[0].module.contains("schema/main.cft"));
    assert!(files.files[0].source.contains("type Item"));

    let _ = std::fs::remove_dir_all(root);
}
