#![allow(clippy::expect_used, clippy::panic, clippy::panic_in_result_fn)]

use coflow_api::SourceLocation;
use coflow_cft::{CftErrorCode, ModuleId};
use coflow_project::{normalize_path, Project};
use coflow_runtime::{ProjectRuntime, ProjectSchemaSession, SchemaTextOverride};
use std::path::PathBuf;

type TestResult = Result<(), String>;

#[test]
fn schema_overrides_match_by_module_or_path_and_reject_unmatched() -> TestResult {
    let (root, project) = test_project("overrides", "type Item { value: string; }")?;
    let schema_path = root.join("schema/main.cft");

    let build = build_schema_attempt(
        project.clone(),
        &[SchemaTextOverride {
            requested_module: Some("schema/main.cft".to_string()),
            normalized_path: normalize_path(&root.join("not-used.cft")),
            source: "type Replacement { value: string; }".to_string(),
        }],
    )?;
    assert!(!build.has_diagnostics());
    assert!(build
        .modules()
        .file(&ModuleId::from("schema/main.cft"))
        .is_some_and(|module| module.source().contains("Replacement")));

    let build = build_schema_attempt(
        project.clone(),
        &[SchemaTextOverride {
            requested_module: None,
            normalized_path: normalize_path(&schema_path),
            source: "type PathReplacement { value: string; }".to_string(),
        }],
    )?;
    assert!(!build.has_diagnostics());
    assert!(build
        .modules()
        .file(&ModuleId::from("schema/main.cft"))
        .is_some_and(|module| module.source().contains("PathReplacement")));

    let err = ProjectRuntime::new(project.clone())
        .refresh_with_overrides(&[SchemaTextOverride {
            requested_module: Some("schema/missing.cft".to_string()),
            normalized_path: normalize_path(&root.join("schema/missing.cft")),
            source: "type Missing { value: string; }".to_string(),
        }])
    .expect_err("unmatched override should fail");
    assert!(err.contains("`--stdin-path schema/missing.cft` is not part"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn invalid_module_keeps_diagnostics_without_compiling() -> TestResult {
    let (root, project) = test_project("invalid", "type Broken { value: Missing; }")?;

    let build = build_schema_attempt(project, &[])?;

    assert!(build.has_diagnostics());
    assert!(build
        .diagnostics()
        .clone()
        .into_set()
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnknownNamedType.as_str()));
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn override_parse_error_keeps_sources_and_paths() -> TestResult {
    let (root, project) = test_project("parse-error", "type Item { value: string; }")?;
    let schema_path = root.join("schema/main.cft");
    let override_source = "type Broken { value: string;".to_string();

    let build = build_schema_attempt(
        project,
        &[SchemaTextOverride {
            requested_module: Some("schema/main.cft".to_string()),
            normalized_path: normalize_path(&schema_path),
            source: override_source.clone(),
        }],
    )?;

    assert!(build.has_diagnostics());
    assert!(build
        .diagnostics()
        .clone()
        .into_set()
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnexpectedEof.as_str()));
    let module = build
        .modules()
        .file(&ModuleId::from("schema/main.cft"))
        .expect("module retained after parse failure");
    assert_eq!(module.source(), override_source);
    assert!(module.path().ends_with("main.cft"));
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_schema_publishes_canonical_utf16_diagnostics() -> TestResult {
    let source = "type 表 {\n  名: Missing;\n}\n";
    let (root, project) = test_project("utf16", source)?;
    let build = build_schema_attempt(project, &[])?;
    let diagnostics = build
        .diagnostics()
        .clone()
        .into_set();
    let converted = diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CftErrorCode::UnknownNamedType.as_str())
        .expect("canonical diagnostic");
    assert!(matches!(
        converted.primary.as_ref().map(|label| &label.location),
        Some(SourceLocation::FileSpan {
            path,
            start_line: 1,
            start_character: 5,
            end_line: 1,
            end_character: 12,
        }) if path.ends_with("schema/main.cft")
    ));
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

fn build_schema_attempt(
    project: Project,
    overrides: &[SchemaTextOverride],
) -> Result<ProjectSchemaSession, String> {
    let mut runtime = ProjectRuntime::new(project);
    let _ = runtime.refresh_with_overrides(overrides);
    runtime
        .into_latest_attempt()
        .ok_or_else(|| "runtime did not retain a schema attempt".to_string())
}

fn test_project(name: &str, source: &str) -> Result<(PathBuf, Project), String> {
    let root = std::env::temp_dir().join(format!(
        "coflow-runtime-schema-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|err| err.to_string())?;
    }
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), source).map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    Ok((root, project))
}
