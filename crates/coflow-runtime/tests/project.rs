#![allow(clippy::expect_used)]

use coflow_runtime::{
    add_project_input, create_project_file, delete_project_entry, init_project, Project,
    ProjectInputKind, DEFAULT_PROJECT_YAML,
};
use std::fs;
use tempfile::TempDir;

fn project_with_config(config: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(dir.path().join("coflow.yaml"), config).expect("config");
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data")).expect("data");
    fs::write(
        dir.path().join("data/items.cfd"),
        "Item { sword { name: \"Fire Sword\", } }\n",
    )
    .expect("data");
    dir
}

#[test]
fn parses_only_data_and_codegen_contract() {
    let project = project_with_config(
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    );
    let opened =
        Project::open_schema_only(Some(&project.path().join("coflow.yaml"))).expect("open project");
    assert_eq!(opened.data_paths().len(), 1);
    assert_eq!(opened.data_paths()[0].path(), std::path::Path::new("data/"));
    assert_eq!(opened.config().codegen.len(), 1);
    assert_eq!(opened.config().codegen[0].language, "csharp");
    assert_eq!(opened.config().codegen[0].options(), &serde_json::json!({}));
}

#[test]
fn rejects_removed_sources_and_outputs_without_fallback() {
    for field in [
        "sources: data/\n",
        "outputs:\n  - type: csharp\n    dir: generated\n",
    ] {
        let config = format!("schema: schema.cft\n{field}");
        let error = serde_yaml::from_str::<coflow_runtime::ProjectConfig>(&config)
            .expect_err("removed field must fail");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}

#[test]
fn rejects_non_path_data_entries() {
    let config = "schema: schema.cft\ndata:\n  - path: data\n    options: {}\n";
    let error = serde_yaml::from_str::<coflow_runtime::ProjectConfig>(config)
        .expect_err("provider-shaped data must fail");
    assert!(error.to_string().contains("CFD paths"), "{error}");
}

#[test]
fn validation_reports_missing_codegen_and_missing_data_path() {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: missing\n",
    )
    .expect("config");
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; }\n",
    )
    .expect("schema");
    let project = Project::open_schema_only(Some(&dir.path().join("coflow.yaml"))).expect("open");
    let diagnostics = project.codegen_diagnostic_set();
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|d| d.message.contains("codegen")));
    let data = project.data_diagnostic_set();
    assert!(data
        .diagnostics
        .iter()
        .any(|d| d.message.contains("does not exist")));
}

#[test]
fn default_init_template_has_no_export_or_provider_fields() {
    assert!(DEFAULT_PROJECT_YAML.contains("data:"));
    assert!(DEFAULT_PROJECT_YAML.contains("codegen:"));
    assert!(!DEFAULT_PROJECT_YAML.contains("sources:"));
    assert!(!DEFAULT_PROJECT_YAML.contains("outputs:"));
}

#[test]
fn init_creates_schema_and_data_directories() {
    let parent = tempfile::tempdir().expect("temp parent");
    let root = parent.path().join("project");
    init_project(&root).expect("init project");
    assert!(root.join("schema").is_dir());
    assert!(root.join("data").is_dir());
}

#[test]
fn adds_existing_inputs_to_project_config() {
    let project = project_with_config(
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    );
    let extra_schema = project.path().join("extra.cft");
    let extra_data = project.path().join("extra.cfd");
    fs::write(&extra_schema, "table Extra {}\n").expect("extra schema");
    fs::write(&extra_data, "").expect("extra data");
    let config = project.path().join("coflow.yaml");

    add_project_input(&config, ProjectInputKind::Schema, &extra_schema).expect("add schema");
    add_project_input(&config, ProjectInputKind::Data, &extra_data).expect("add data");

    let opened = Project::open_schema_only(Some(&config)).expect("reopen project");
    assert_eq!(opened.config().schema.paths().len(), 2);
    assert!(opened
        .data_paths()
        .iter()
        .any(|source| source.path() == std::path::Path::new("extra.cfd")));
}

#[test]
fn rejects_configured_inputs_that_contain_one_another() {
    let project = project_with_config("schema: schema.cft\ndata: data/\n");
    let nested = project.path().join("data/items.cfd");
    let error = add_project_input(
        &project.path().join("coflow.yaml"),
        ProjectInputKind::Data,
        &nested,
    )
    .expect_err("nested input must be rejected");
    assert!(error
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PROJECT-CONFIG-OVERLAP"));
}

#[test]
fn external_config_paths_are_saved_relative_for_every_directory_kind() {
    let project = project_with_config("schema: schema.cft\ndata: data/\n");
    let parent = project.path().parent().expect("parent");
    let external = tempfile::Builder::new()
        .prefix("coflow-shared-")
        .tempdir_in(parent)
        .expect("external");
    let schema = external.path().join("extra.cft");
    fs::write(&schema, "table Extra {}\n").expect("schema");
    let config_path = project.path().join("coflow.yaml");
    fs::write(
        &config_path,
        serde_yaml::to_string(&serde_json::json!({
            "schema": project.path().join("schema.cft"),
            "data": project.path().join("data"),
            "dimensions": {"language": {"dir": external.path().join("translations")}},
            "codegen": [{"language": "csharp", "dir": external.path().join("generated")}]
        }))
        .expect("yaml"),
    )
    .expect("config");
    add_project_input(&config_path, ProjectInputKind::Schema, &schema).expect("add external");
    let opened = Project::open_schema_only(Some(&config_path)).expect("open");
    assert!(opened
        .config()
        .schema
        .paths()
        .iter()
        .all(|p| p.is_relative()));
    assert!(opened.config().data.iter().all(|p| p.path().is_relative()));
    assert!(opened.config().codegen[0].dir.is_relative());
    assert!(opened.config().dimensions["language"]
        .dir
        .as_ref()
        .expect("dir")
        .is_relative());
    assert_eq!(
        coflow_runtime::normalize_path(&opened.resolve_path(&opened.config().codegen[0].dir)),
        coflow_runtime::normalize_path(&external.path().join("generated"))
    );
    assert_eq!(
        coflow_runtime::normalize_path(
            &opened.resolve_path(
                opened
                    .config()
                    .schema
                    .paths()
                    .last()
                    .expect("external schema")
            )
        ),
        coflow_runtime::normalize_path(&schema)
    );
}

#[test]
fn creates_nested_file_and_recursively_deletes_configured_directory() {
    let project = project_with_config("schema: schema.cft\ndata: data/\n");
    let config = project.path().join("coflow.yaml");
    fs::create_dir_all(project.path().join("data/nested")).expect("nested data directory");

    create_project_file(
        &config,
        ProjectInputKind::Data,
        std::path::Path::new("data/nested"),
        "new.cfd",
    )
    .expect("create nested CFD");
    assert!(project.path().join("data/nested/new.cfd").is_file());

    delete_project_entry(&config, std::path::Path::new("data"))
        .expect("delete configured data directory");
    assert!(!project.path().join("data").exists());
    let reopened = Project::open_schema_only(Some(&config)).expect("reopen without data root");
    assert!(reopened.data_paths().is_empty());
}

#[test]
fn absolute_config_paths_are_converted_on_read_and_saved_relatively() {
    let dir = project_with_config("");
    let root = coflow_runtime::canonicalize_path(dir.path()).expect("root");
    let absolute = |suffix: &str| root.join(suffix).to_string_lossy().replace('\\', "/");
    let source = format!(
        "schema: '{}'\ndata: '{}'\ncodegen: {{ language: csharp, dir: '{}' }}\ndimensions:\n  language:\n    dir: '{}'\n",
        absolute("schema.cft"), absolute("data"), absolute("generated"), absolute("dimensions/language")
    );
    let config = root.join("coflow.yaml");
    fs::write(&config, &source).expect("absolute config");
    let project = Project::open_schema_only(Some(&config)).expect("read config");
    let encoded = serde_yaml::to_string(project.config()).expect("encode config");
    assert!(!encoded.contains(&absolute("")));
    assert_eq!(
        project.config().schema.paths()[0],
        std::path::Path::new("schema.cft")
    );
    assert_eq!(
        project.config().data[0].path(),
        std::path::Path::new("data")
    );
    assert_eq!(
        project.config().codegen[0].dir,
        std::path::Path::new("generated")
    );
    assert_eq!(
        project.config().dimensions["language"].dir.as_deref(),
        Some(std::path::Path::new("dimensions/language"))
    );
    assert_eq!(
        fs::read_to_string(&config).expect("unchanged config"),
        source
    );
    fs::write(root.join("extra.cfd"), "").expect("extra input");
    add_project_input(&config, ProjectInputKind::Data, &root.join("extra.cfd"))
        .expect("save config");
    let saved = fs::read_to_string(config).expect("saved config");
    assert!(!saved.contains(&absolute("")));
}
