#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes
)]

use coflow_api::WriteFieldPathSegment;
use coflow_cft::{CftContainer, Dimension, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputRecord, CfdInputValue, CfdValue};
use coflow_engine::{build_project_session, build_project_session_read_only, ProjectSession};
use coflow_project::Project;

fn csv_dimension_registry() -> coflow_api::ProviderRegistry {
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_source_provider(coflow_loader_csv::CsvLoader)
        .expect("csv loader");
    registry
        .register_dimension_source_manager(coflow_loader_csv::CsvWriter::new())
        .expect("csv dimension source manager");
    registry
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
    assert!(field
        .dimension
        .as_ref()
        .is_some_and(|dimension| matches!(dimension.kind, Dimension::Localized)));
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

#[test]
fn localized_schema_requires_language_dimension_config() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-config-missing-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/main.cft\nsources: []\n",
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_api::ProviderRegistry::default();
    let session = build_project_session(project, &registry).expect("build session");

    assert!(
        session
            .diagnostics
            .as_set()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "DIM-CONFIG-001"
                && diagnostic.message
                    == "schema contains @localized fields but dimensions.language is not configured"
            }),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn custom_dimension_schema_requires_matching_dimension_config() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-custom-dim-config-missing-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @dimension("platform")
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/main.cft\nsources: []\n",
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_api::ProviderRegistry::default();
    let session = build_project_session(project, &registry).expect("build session");

    assert!(
        session
            .diagnostics
            .as_set()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "DIM-CONFIG-001"
                    && diagnostic.message
                        == "schema contains @dimension(\"platform\") fields but dimensions.platform is not configured"
                    && diagnostic.primary.as_ref().is_some_and(|label| {
                        matches!(
                            &label.location,
                            coflow_api::SourceLocation::ProjectConfig { key_path, .. }
                                if key_path == &vec![
                                    "dimensions".to_string(),
                                    "platform".to_string()
                                ]
                        )
                    })
            }),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_injects_variant_type_and_implicit_sources() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-synthesis-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh,en\npotion,Potion,药水,Potion\n",
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources: []
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");

    let variants = session
        .schema
        .resolve_type("Item_nameVariants")
        .expect("synthesized type");
    assert_eq!(
        variants
            .all_fields
            .iter()
            .map(|field| (field.name.as_str(), &field.ty_ref))
            .collect::<Vec<_>>(),
        vec![
            (
                "default",
                &coflow_cft::CftSchemaTypeRef::Nullable(Box::new(
                    coflow_cft::CftSchemaTypeRef::String
                ))
            ),
            (
                "zh",
                &coflow_cft::CftSchemaTypeRef::Nullable(Box::new(
                    coflow_cft::CftSchemaTypeRef::String
                ))
            ),
            (
                "en",
                &coflow_cft::CftSchemaTypeRef::Nullable(Box::new(
                    coflow_cft::CftSchemaTypeRef::String
                ))
            ),
        ]
    );
    assert!(session
        .files
        .source_files()
        .contains("data/dimensions/language/Item_name.csv"));

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn custom_dimension_injects_variant_type_and_implicit_sources() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-custom-dim-synthesis-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("data/dimensions/platform")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @dimension("platform")
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n")
        .expect("write source csv");
    std::fs::write(
        root.join("data/dimensions/platform/Item_name.csv"),
        "id,default,pc,mobile\npotion,Potion,PC Potion,Mobile Potion\n",
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  platform:
    variants: [pc, mobile]
    out_dir: data/dimensions/platform
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");

    let variants = session
        .schema
        .resolve_type("Item_nameVariants")
        .expect("synthesized type");
    assert_eq!(
        variants
            .all_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec!["default", "pc", "mobile"]
    );
    assert!(session
        .files
        .source_files()
        .contains("data/dimensions/platform/Item_name.csv"));
    assert!(session
        .records
        .get_by_coordinate("Item_nameVariants", "potion")
        .is_some());

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_synthesizes_nullable_source_fields_once() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-nullable-synthesis-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string?;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources: []
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");

    let variants = session
        .schema
        .resolve_type("Item_nameVariants")
        .expect("synthesized type");
    assert_eq!(variants.all_fields[0].ty, "string?");
    assert_eq!(variants.all_fields[1].ty, "string?");

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn read_only_session_does_not_generate_dimension_sources() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-read-only-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    sheets:
      - sheet: Item
        type: Item
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session_read_only(project, &registry).expect("build session");

    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );
    assert!(
        !root.join("data/dimensions/language/Item_name.csv").exists(),
        "read-only session must not create dimension source files"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn inherited_localized_fields_are_not_synthesized_for_child_types() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-inherited-field-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Base {
            @localized
            name: string;
            check { name != ""; }
        }

        type Child : Base {
            power: int;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/children.csv"),
        "id,name,power\nchild,Potion,1\n",
    )
    .expect("write child source");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/children.csv
    type: csv
    sheets:
      - sheet: children
        type: Child
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    assert!(session.schema.resolve_type("Base_nameVariants").is_some());
    assert!(session.schema.resolve_type("Child_nameVariants").is_none());
    assert!(root.join("data/dimensions/language/Base_name.csv").exists());
    assert!(
        !root
            .join("data/dimensions/language/Child_name.csv")
            .exists(),
        "inherited localized fields should not generate child dimension files"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_regenerates_csv_with_defaults_and_preserved_variants() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-regenerate-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh,en\npotion,Old,药水,\n",
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated = std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
        .expect("read generated dimension csv");
    assert_eq!(generated, "id,default,zh,en\npotion,Potion,药水,null\n");

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_regenerates_csv_removes_stale_records() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-remove-stale-records-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh,old\npotion,Old,药水,legacy\nstale,Stale,旧,legacy\n",
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated = std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
        .expect("read generated dimension csv");
    assert_eq!(generated, "id,default,zh\npotion,Potion,药水\n");

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_rolls_back_generated_csv_when_reload_checks_fail() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-rollback-check-failure-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;

            check { name != "BAD"; }
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    let original_dimension_csv = "id,default,zh\npotion,Old,BAD\n";
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        original_dimension_csv,
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        session.has_diagnostics(),
        "dimension zh variant should fail the check"
    );
    assert!(
        session
            .diagnostics
            .as_set()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("[language=zh]")),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated = std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
        .expect("read rolled back dimension csv");
    assert_eq!(generated, original_dimension_csv);

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_does_not_rewrite_unchanged_generated_files() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-no-unchanged-rewrite-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let registry = csv_dimension_registry();
    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated_path = root.join("data/dimensions/language/Item_name.csv");
    let first_modified = std::fs::metadata(&generated_path)
        .expect("metadata")
        .modified()
        .expect("modified time");
    std::thread::sleep(std::time::Duration::from_millis(1200));

    let project = Project::open_schema_only(Some(&root)).expect("reopen project");
    let session = build_project_session(project, &registry).expect("rebuild session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );
    let second_modified = std::fs::metadata(&generated_path)
        .expect("metadata")
        .modified()
        .expect("modified time");

    assert_eq!(
        first_modified, second_modified,
        "unchanged generated dimension file should not be rewritten"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_uses_bucket_for_csv_file_names() {
    let root =
        std::env::temp_dir().join(format!("coflow-engine-dim-bucket-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized(bucket = "ui")
            icon: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,icon\npotion,Icon\n").expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    assert!(session
        .files
        .source_files()
        .contains("data/dimensions/language/ui_icon.csv"));
    let generated = std::fs::read_to_string(root.join("data/dimensions/language/ui_icon.csv"))
        .expect("read generated dimension csv");
    assert_eq!(generated, "id,default,zh\npotion,Icon,null\n");
    assert!(
        !root.join("data/dimensions/language/Item_icon.csv").exists(),
        "bucketed fields should not use source type csv name"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_regenerates_csv_defaults_with_cell_value_syntax() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-regenerate-cell-values-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;

            @localized
            tags: [string];
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"potion: Item {
    name: "Potion, Large",
    tags: ["healing", "fast, use"],
}
"#,
    )
    .expect("write source");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.cfd
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default provider registry");
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated_name =
        std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
            .expect("read generated name csv");
    let generated_tags =
        std::fs::read_to_string(root.join("data/dimensions/language/Item_tags.csv"))
            .expect("read generated tags csv");
    assert_eq!(
        generated_name,
        "id,default,zh\npotion,\"\"\"Potion, Large\"\"\",null\n"
    );
    assert_eq!(
        generated_tags,
        "id,default,zh\npotion,\"[healing | \"\"fast, use\"\"]\",null\n"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn language_dimension_regenerates_singleton_cfd_with_defaults_and_preserved_variants() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-regenerate-singleton-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        @singleton
        type UiText {
            @localized
            welcome: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/ui.cfd"),
        r#"UiText: UiText {
    welcome: "Welcome"
}
"#,
    )
    .expect("write singleton source");
    std::fs::write(
        root.join("data/dimensions/language/UiText.cfd"),
        r#"welcome: UiText_welcomeVariants {
    default: "Old",
    zh: "欢迎",
    en: null,
}
"#,
    )
    .expect("write dimension cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/ui.cfd
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default provider registry");
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    let generated = std::fs::read_to_string(root.join("data/dimensions/language/UiText.cfd"))
        .expect("read generated dimension cfd");
    assert_eq!(
        generated,
        "welcome: UiText_welcomeVariants {\n    default: \"Welcome\",\n    zh: \"欢迎\",\n    en: null,\n}\n\n"
    );
    assert!(session
        .files
        .source_files()
        .contains("data/dimensions/language/UiText.cfd"));

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

/// Regression: spec 17 §1.1 — source record `Item.potion` and synthetic
/// dimension record `Item_nameVariants.potion` share the record key `potion`
/// but live in different types. The pre-refactor `RecordIndex` keyed records
/// by bare `key`, so the second `add(potion)` clobbered the first and
/// `keys_for_file("Item_name.csv")` returned the wrong record's fields.
///
/// After Phase 2, `RecordIndex` is keyed by `(actual_type, key)`. Both
/// records coexist; `ids_in_file("Item_name.csv")` lists only the synthetic
/// row and `record.actual_type` resolves to `Item_nameVariants` (not `Item`).
#[test]
fn synthetic_and_source_records_with_same_key_do_not_collide() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-record-coordinate-collision-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            @localized
            name: string;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = csv_dimension_registry();
    let session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    // Both records exist in the model, each addressable by (type, key).
    let source = session
        .records
        .get_by_coordinate("Item", "potion")
        .expect("source `Item.potion` should be indexed");
    let synthetic = session
        .records
        .get_by_coordinate("Item_nameVariants", "potion")
        .expect("synthetic `Item_nameVariants.potion` should be indexed");
    assert_ne!(source.id, synthetic.id, "both records have distinct ids");
    assert_eq!(source.display_path, "data/items.csv");
    assert_eq!(
        synthetic.display_path,
        "data/dimensions/language/Item_name.csv"
    );

    // The synthetic file lists only the synthetic record — the source row's
    // fields must not bleed through.
    let ids_in_variants_file = session
        .records
        .ids_in_file("data/dimensions/language/Item_name.csv")
        .to_vec();
    assert_eq!(
        ids_in_variants_file,
        vec![synthetic.id],
        "synthetic file index should hold only the variant record"
    );
    let coordinate = session
        .records
        .get(synthetic.id)
        .expect("synthetic record ref")
        .coordinate
        .clone();
    assert_eq!(coordinate.actual_type, "Item_nameVariants");
    assert_eq!(coordinate.key, "potion");

    // `record_view` returns the synthetic record's fields when addressed by
    // its coordinate — not the source `Item` record's fields.
    let view = session
        .record_view("Item_nameVariants", "potion")
        .expect("record view");
    assert!(view.record.fields().contains_key("default"));
    assert!(view.record.fields().contains_key("zh"));
    assert!(view.record.fields().contains_key("en"));
    assert!(!view.record.fields().contains_key("name"));

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

#[test]
fn write_field_redirects_spread_fields_to_source_record() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-spread-write-source-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item {
            name: string;
            power: int;
        }
        type Holder {
            stats: Item;
        }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/source.cfd"),
        r#"base: Item {
    name: "Base",
    power: 1,
}
"#,
    )
    .expect("write source");
    std::fs::write(
        root.join("data/host.cfd"),
        r#"child: Item {
    ...&base,
}
holder: Holder {
    stats: {
        ...&base,
    },
}
chain: Holder {
    stats: {
        ...&child,
    },
}
"#,
    )
    .expect("write host");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/source.cfd
  - path: data/host.cfd
"#,
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default provider registry");
    let mut session = build_project_session(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics: {:?}",
        session.diagnostics.as_set()
    );

    session
        .write_field(
            &registry,
            "Item",
            "child",
            &[WriteFieldPathSegment::Field("name".to_string())],
            &CfdValue::String("Edited".to_string()),
        )
        .expect("spread field write");

    let source = std::fs::read_to_string(root.join("data/source.cfd")).expect("read source");
    let host = std::fs::read_to_string(root.join("data/host.cfd")).expect("read host");
    assert!(
        source.contains(r#"name: "Edited""#),
        "source file should receive spread edit:\n{source}"
    );
    assert!(
        host.contains("...&base") && !host.contains("Edited"),
        "host file should not receive spread edit:\n{host}"
    );

    assert_nested_spread_write_redirects(&mut session, &registry, &root);

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

fn assert_nested_spread_write_redirects(
    session: &mut ProjectSession,
    registry: &coflow_api::ProviderRegistry,
    root: &std::path::Path,
) {
    session
        .write_field(
            registry,
            "Holder",
            "holder",
            &[
                WriteFieldPathSegment::Field("stats".to_string()),
                WriteFieldPathSegment::Field("name".to_string()),
            ],
            &CfdValue::String("Nested".to_string()),
        )
        .expect("nested spread field write");

    let source = std::fs::read_to_string(root.join("data/source.cfd")).expect("read source");
    let host = std::fs::read_to_string(root.join("data/host.cfd")).expect("read host");
    assert!(
        source.contains(r#"name: "Nested""#),
        "source file should receive nested spread edit:\n{source}"
    );
    assert!(
        host.contains("stats") && !host.contains("Nested"),
        "host file should not receive nested spread edit:\n{host}"
    );

    session
        .write_field(
            registry,
            "Holder",
            "chain",
            &[
                WriteFieldPathSegment::Field("stats".to_string()),
                WriteFieldPathSegment::Field("name".to_string()),
            ],
            &CfdValue::String("Chained".to_string()),
        )
        .expect("chained spread field write");

    let source = std::fs::read_to_string(root.join("data/source.cfd")).expect("read source");
    let host = std::fs::read_to_string(root.join("data/host.cfd")).expect("read host");
    assert!(
        source.contains(r#"name: "Chained""#),
        "source file should receive chained spread edit:\n{source}"
    );
    assert!(
        !host.contains("Chained"),
        "host file should not receive chained spread edit:\n{host}"
    );
}

#[test]
fn rename_record_updates_direct_refs_and_spread_sources_without_global_ref_scan() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-rename-spread-source-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    write_rename_spread_project(&root);

    let project = Project::open_schema_only(Some(&root)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default provider registry");
    let mut session = build_project_session(project, &registry).expect("build session");

    session
        .rename_record_key(&registry, "Holder", "base_holder", "renamed_holder")
        .expect("rename base holder");

    let items = std::fs::read_to_string(root.join("data/items.cfd")).expect("read items");
    let host = std::fs::read_to_string(root.join("data/host.cfd")).expect("read host");
    let unrelated =
        std::fs::read_to_string(root.join("data/unrelated.cfd")).expect("read unrelated");

    assert!(
        host.contains("renamed_holder: Holder"),
        "host record renamed:\n{host}"
    );
    assert!(
        items.contains("base: Item"),
        "item source unchanged:\n{items}"
    );
    assert!(
        host.contains("holder: &renamed_holder") && host.contains("...&renamed_holder"),
        "direct Holder refs and selected spread source should update:\n{host}"
    );
    assert!(
        host.contains(r#"label: "&base""#),
        "string literal should not be rewritten:\n{host}"
    );
    assert!(
        host.contains("same_file_unrelated: OtherHolder {\n    ...&base_holder"),
        "same-file unrelated same-key spread should not be rewritten by a source scan:\n{host}"
    );
    assert!(
        unrelated.contains("item: &other") && unrelated.contains(r#"label: "&base""#),
        "unrelated source should not be globally scanned:\n{unrelated}"
    );

    std::fs::remove_dir_all(root).expect("remove temp dir");
}

fn write_rename_spread_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
        type Item { name: string; }
        type Holder { item: &Item; label: string; }
        type Wrapper { holder: &Holder; label: string; }
        type OtherHolder { item: &Item; label: string; }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"base: Item {
    name: "Base",
}
other: Item {
    name: "Other",
}
"#,
    )
    .expect("write items");
    std::fs::write(root.join("data/host.cfd"), rename_spread_host_source()).expect("write host");
    std::fs::write(
        root.join("data/unrelated.cfd"),
        r#"unrelated: Holder {
    item: &other,
    label: "&base",
}
"#,
    )
    .expect("write unrelated");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/items.cfd
  - path: data/host.cfd
  - path: data/unrelated.cfd
"#,
    )
    .expect("write config");
}

const fn rename_spread_host_source() -> &'static str {
    r#"base_holder: Holder {
    item: &base,
    label: "&base",
}
direct: Holder {
    item: &base,
    label: "&base",
}
copy: Holder {
    ...&base_holder,
}
direct_wrapper: Wrapper {
    holder: &base_holder,
    label: "&base_holder",
}
base_holder: OtherHolder {
    item: &base,
    label: "Other",
}
same_file_unrelated: OtherHolder {
    ...&base_holder,
}
"#
}
