#![allow(clippy::expect_used, clippy::panic)]

use coflow_engine::{build_project_schema_session, inspect_schema, schema_files};
use coflow_project::Project;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @display("Item type")
            @idAsEnum(ItemId)
            type Item {
                @display("Display name")
                name: string;
                rarity: Rarity = Rarity.Common;
            }

            @display("Rarity enum")
            enum Rarity {
                @display("Common rarity")
                Common = 0,
                Rare = 10,
            }

            enum ItemId {}
        "#,
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

#[test]
fn inspect_schema_preserves_annotations_fields_and_enums() {
    let root = std::env::temp_dir().join(format!("coflow-schema-inspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_project_schema_session(project).expect("schema session");
    let report = inspect_schema(&session, None, false);

    let item = report
        .types
        .iter()
        .find(|ty| ty.name == "Item")
        .expect("Item type");
    assert!(item.annotations.iter().any(|a| a.name == "display"));
    assert!(item.annotations.iter().any(|a| a.name == "idAsEnum"));
    assert!(item.fields.iter().any(|field| {
        field.name == "name" && field.annotations.iter().any(|a| a.name == "display")
    }));
    assert!(report.enums.iter().any(|e| {
        e.name == "Rarity"
            && e.annotations.iter().any(|a| a.name == "display")
            && e.variants
                .iter()
                .any(|v| v.name == "Common" && v.annotations.iter().any(|a| a.name == "display"))
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn inspect_schema_serializes_large_i64_values_as_strings() {
    let root = std::env::temp_dir().join(format!("coflow-schema-i64-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_large_i64_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_project_schema_session(project).expect("schema session");
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
fn schema_files_returns_compiled_module_sources() {
    let root = std::env::temp_dir().join(format!("coflow-schema-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_project_schema_session(project).expect("schema session");
    let files = schema_files(&session);

    assert_eq!(files.files.len(), 1);
    assert!(files.files[0].module.contains("schema/main.cft"));
    assert!(files.files[0].source.contains("type Item"));

    let _ = std::fs::remove_dir_all(root);
}
