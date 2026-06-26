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
