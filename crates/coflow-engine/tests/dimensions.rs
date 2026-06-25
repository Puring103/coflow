#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes
)]

use coflow_cft::{CftContainer, Dimension, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputRecord, CfdInputValue};
use coflow_engine::build_project_session;
use coflow_project::Project;

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
fn language_dimension_injects_variant_type_and_implicit_sources() {
    let root = std::env::temp_dir().join(format!(
        "coflow-engine-dim-synthesis-{}",
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
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_csv::CsvLoader)
        .expect("csv loader");
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
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_csv::CsvLoader)
        .expect("csv loader");
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
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_cfd::CfdLoader)
        .expect("cfd loader");
    registry
        .register_loader(coflow_loader_csv::CsvLoader)
        .expect("csv loader");
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
    assert_eq!(generated_name, "id,default,zh\npotion,\"\"\"Potion, Large\"\"\",null\n");
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
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_cfd::CfdLoader)
        .expect("cfd loader");
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
