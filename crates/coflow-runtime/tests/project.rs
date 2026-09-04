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
        "type Item { name: string; }\n",
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
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n",
    );
    let opened =
        Project::open_schema_only(Some(&project.path().join("coflow.yaml"))).expect("open project");
    assert_eq!(opened.data_paths().len(), 1);
    assert_eq!(opened.data_paths()[0].path(), std::path::Path::new("data/"));
    assert_eq!(opened.config().codegen.len(), 1);
    assert_eq!(opened.config().codegen[0].language, "csharp");
    assert_eq!(
        opened.config().codegen[0].options()["namespace"],
        "Game.Config"
    );
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
        "type Item { name: string; }\n",
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
    fs::write(&extra_schema, "type Extra {}\n").expect("extra schema");
    fs::write(&extra_data, "").expect("extra data");
    let config = project.path().join("coflow.yaml");

    add_project_input(&config, ProjectInputKind::Schema, &extra_schema).expect("add schema");
    add_project_input(&config, ProjectInputKind::Data, &extra_data).expect("add data");

    let opened = Project::open_schema_only(Some(&config)).expect("reopen project");
    assert_eq!(opened.config().schema.paths().len(), 2);
    assert!(opened.data_paths().iter().any(|source| source.path() == std::path::Path::new("extra.cfd")));
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
    assert!(error.diagnostics.iter().any(|diagnostic| diagnostic.code == "PROJECT-CONFIG-OVERLAP"));
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
