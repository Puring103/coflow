#![allow(clippy::expect_used, clippy::panic, clippy::panic_in_result_fn)]

use coflow_cft::CftErrorCode;
use coflow_project::{normalize_path, Project};
use coflow_runtime::{compile_schema_project_with_overrides, SchemaSourceOverride};
use std::path::PathBuf;

type TestResult = Result<(), String>;

#[test]
fn schema_overrides_match_by_module_or_path_and_reject_unmatched() -> TestResult {
    let (root, project) = test_project("overrides", "type Item { value: string; }")?;
    let schema_path = root.join("schema/main.cft");

    let build = compile_schema_project_with_overrides(
        &project,
        &[SchemaSourceOverride {
            requested_module: Some("schema/main.cft".to_string()),
            normalized_path: normalize_path(&root.join("not-used.cft")),
            source: "type Replacement { value: string; }".to_string(),
        }],
    )
    .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("Replacement"));

    let build = compile_schema_project_with_overrides(
        &project,
        &[SchemaSourceOverride {
            requested_module: None,
            normalized_path: normalize_path(&schema_path),
            source: "type PathReplacement { value: string; }".to_string(),
        }],
    )
    .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("PathReplacement"));

    let err = compile_schema_project_with_overrides(
        &project,
        &[SchemaSourceOverride {
            requested_module: Some("schema/missing.cft".to_string()),
            normalized_path: normalize_path(&root.join("schema/missing.cft")),
            source: "type Missing { value: string; }".to_string(),
        }],
    )
    .expect_err("unmatched override should fail");
    assert!(err.contains("`--stdin-path schema/missing.cft` is not part"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn invalid_module_keeps_diagnostics_without_compiling() -> TestResult {
    let (root, project) = test_project("invalid", "type Broken { value: Missing; }")?;

    let build = compile_schema_project_with_overrides(&project, &[])
        .map_err(|err| err.to_string())?;

    assert!(build.container.is_none());
    assert!(build
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnknownNamedType));
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn override_parse_error_keeps_sources_and_paths() -> TestResult {
    let (root, project) = test_project("parse-error", "type Item { value: string; }")?;
    let schema_path = root.join("schema/main.cft");
    let override_source = "type Broken { value: string;".to_string();

    let build = compile_schema_project_with_overrides(
        &project,
        &[SchemaSourceOverride {
            requested_module: Some("schema/main.cft".to_string()),
            normalized_path: normalize_path(&schema_path),
            source: override_source.clone(),
        }],
    )
    .map_err(|err| err.to_string())?;

    assert!(build.container.is_none());
    assert!(build
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnexpectedEof));
    assert_eq!(build.sources["schema/main.cft"], override_source);
    assert!(build.paths["schema/main.cft"].ends_with("main.cft"));
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
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
