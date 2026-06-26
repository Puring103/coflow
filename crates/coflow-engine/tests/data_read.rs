#![allow(clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use coflow_engine::{
    build_project_session, data_get, data_list, data_sources, DataGetQuery, DataListQuery,
    RecordCoordinate,
};
use coflow_project::Project;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
                price: int;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            sword: Item { name: "Sword", price: 100 }
            shield: Item { name: "Shield", price: 80 }
        "#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_large_project(root: &std::path::Path, count: usize) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
                price: int;
            }
        ",
    )
    .expect("write schema");
    let mut records = String::new();
    for index in 0..count {
        writeln!(
            records,
            "item_{index}: Item {{ name: \"Item {index}\", price: {index} }}"
        )
        .expect("write record text");
    }
    std::fs::write(root.join("data").join("items.cfd"), records).expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn registry() -> coflow_api::ProviderRegistry {
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_cfd::CfdLoader)
        .expect("cfd loader");
    registry
        .register_writer(coflow_loader_cfd::CfdWriter::new())
        .expect("cfd writer");
    registry
}

#[test]
fn data_sources_report_provider_capabilities_and_types() {
    let root = std::env::temp_dir().join(format!("coflow-data-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");

    let report = data_sources(&session, &registry);
    let source = report
        .sources
        .iter()
        .find(|source| source.file == "data/items.cfd")
        .expect("items source");
    assert_eq!(source.provider, "cfd");
    assert_eq!(source.capabilities.provider_id, "cfd");
    assert!(source.capabilities.can_edit_field);
    assert!(source.capabilities.can_insert_record);
    assert_eq!(source.types, vec!["Item"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_list_filters_and_paginates_record_summaries() {
    let root = std::env::temp_dir().join(format!("coflow-data-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");

    let list = data_list(
        &session,
        &DataListQuery {
            actual_type: Some("Item".to_string()),
            file: Some("data/items.cfd".to_string()),
            limit: Some(1),
            offset: 1,
        },
    );

    assert_eq!(list.records.len(), 1);
    assert_eq!(list.records[0].record.key, "shield");
    assert_eq!(list.records[0].file, "data/items.cfd");
    assert_eq!(list.records[0].provider, "cfd");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_supports_selector_and_key_filters() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");

    let selected = data_get(
        &session,
        &DataGetQuery {
            selector: Some(RecordCoordinate::new("Item", "sword")),
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("get selected");
    assert_eq!(selected.records.len(), 1);
    assert_eq!(selected.records[0].record.key, "sword");
    assert_eq!(selected.records[0].file, "data/items.cfd");
    assert!(selected.records[0].fields.contains_key("price"));

    let filtered = data_get(
        &session,
        &DataGetQuery {
            selector: None,
            actual_type: Some("Item".to_string()),
            file: Some("data/items.cfd".to_string()),
            keys: vec!["shield".to_string()],
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("get filtered");
    assert_eq!(filtered.records.len(), 1);
    assert_eq!(filtered.records[0].record.key, "shield");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_returns_diagnostic_for_missing_selector() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");

    let diagnostics = data_get(
        &session,
        &DataGetQuery {
            selector: Some(RecordCoordinate::new("Item", "missing")),
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect_err("missing record should fail");

    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DATA-NOT-FOUND"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_requires_limit_or_all_for_large_unselected_results() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_large_project(&root, 101);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_project_session(project, &registry).expect("session");

    let diagnostics = data_get(
        &session,
        &DataGetQuery {
            selector: None,
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect_err("large unselected result should require limit or all");

    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DATA-GET-LIMIT"));

    let limited = data_get(
        &session,
        &DataGetQuery {
            selector: None,
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: Some(2),
            offset: 0,
            all: false,
        },
    )
    .expect("limited get");
    assert_eq!(limited.records.len(), 2);

    let _ = std::fs::remove_dir_all(root);
}
