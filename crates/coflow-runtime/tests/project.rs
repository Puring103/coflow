use coflow_runtime::{Project, DEFAULT_PROJECT_YAML};
use std::fs;
use tempfile::TempDir;

fn project_with_config(config: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(dir.path().join("coflow.yaml"), config).expect("config");
    fs::write(dir.path().join("schema.cft"), "type Item { name: string; }\n")
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
    let opened = Project::open_schema_only(Some(&project.path().join("coflow.yaml")))
        .expect("open project");
    assert_eq!(opened.data_paths().len(), 1);
    assert_eq!(opened.data_paths()[0].path(), std::path::Path::new("data/"));
    assert_eq!(opened.config().codegen.len(), 1);
    assert_eq!(opened.config().codegen[0].language, "csharp");
    assert_eq!(opened.config().codegen[0].options()["namespace"], "Game.Config");
}

#[test]
fn rejects_removed_sources_and_outputs_without_fallback() {
    for field in ["sources: data/\n", "outputs:\n  - type: csharp\n    dir: generated\n"] {
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
    fs::write(dir.path().join("coflow.yaml"), "schema: schema.cft\ndata: missing\n")
        .expect("config");
    fs::write(dir.path().join("schema.cft"), "type Item { name: string; }\n").expect("schema");
    let project = Project::open_schema_only(Some(&dir.path().join("coflow.yaml"))).expect("open");
    let diagnostics = project.codegen_diagnostic_set();
    assert!(diagnostics.diagnostics.iter().any(|d| d.message.contains("codegen")));
    let data = project.data_diagnostic_set();
    assert!(data.diagnostics.iter().any(|d| d.message.contains("does not exist")));
}

#[test]
fn default_init_template_has_no_export_or_provider_fields() {
    assert!(DEFAULT_PROJECT_YAML.contains("data:"));
    assert!(DEFAULT_PROJECT_YAML.contains("codegen:"));
    assert!(!DEFAULT_PROJECT_YAML.contains("sources:"));
    assert!(!DEFAULT_PROJECT_YAML.contains("outputs:"));
}
