#![allow(
    clippy::expect_used,
    clippy::implicit_clone,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_clone,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_api::SourceLocation;
use coflow_cft::{CftDiagnostic, CftErrorCode, ModuleId, Span};
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, init_project, normalize_path,
    path_to_slash, resolve_config_path, DiagnosticJson, OutputConfig, OutputsConfig, Project,
    ProjectConfig, SchemaConfig, SchemaSourceOverride, DEFAULT_PROJECT_YAML,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type TestResult = Result<(), String>;

#[test]
fn resolve_config_path_rejects_ambiguous_and_invalid_inputs() -> TestResult {
    let root = temp_project_dir("coflow-project-resolve-config");
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;

    let err = resolve_config_path(Some(&root)).expect_err("ambiguous config should fail");
    assert!(err.contains("both `"));
    assert!(err.contains("coflow.yaml"));
    assert!(err.contains("coflow.yml"));

    let missing_yaml = root.join("missing.yaml");
    assert_eq!(
        resolve_config_path(Some(&missing_yaml)).map_err(|err| err.to_string())?,
        missing_yaml
    );

    let missing_dir = root.join("missing-project");
    let err = resolve_config_path(Some(&missing_dir)).expect_err("missing non-yaml should fail");
    assert!(err.contains("config or directory"));

    let not_yaml = root.join("plain.txt");
    std::fs::write(&not_yaml, "").map_err(|err| err.to_string())?;
    assert_eq!(
        resolve_config_path(Some(&not_yaml)).map_err(|err| err.to_string())?,
        not_yaml
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn resolve_config_path_accepts_yml_defaults_and_reports_missing_default_config() -> TestResult {
    let root = temp_project_dir("coflow-project-resolve-defaults");

    let err = resolve_config_path(Some(&root)).expect_err("empty project dir should fail");
    assert!(err.contains("no coflow.yaml or coflow.yml found"));

    let yml = root.join("coflow.yml");
    std::fs::write(&yml, "schema: schema/main.cft\n").map_err(|err| err.to_string())?;
    assert_eq!(
        resolve_config_path(Some(&root)).map_err(|err| err.to_string())?,
        yml
    );

    let explicit_missing_yml = root.join("missing.yml");
    assert_eq!(
        resolve_config_path(Some(&explicit_missing_yml)).map_err(|err| err.to_string())?,
        explicit_missing_yml
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn resolve_config_path_accepts_explicit_directory_with_yaml_default() -> TestResult {
    let root = temp_project_dir("coflow-project-resolve-explicit-dir");
    let yaml = root.join("coflow.yaml");
    std::fs::write(&yaml, "schema: schema/main.cft\n").map_err(|err| err.to_string())?;

    assert_eq!(
        resolve_config_path(Some(&root)).map_err(|err| err.to_string())?,
        yaml
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn resolve_config_path_uses_current_directory_when_no_path_is_given() -> TestResult {
    let original_dir = std::env::current_dir().map_err(|err| err.to_string())?;
    let root = temp_project_dir("coflow-project-resolve-current-dir");

    std::env::set_current_dir(&root).map_err(|err| err.to_string())?;
    let result = (|| -> TestResult {
        let missing = resolve_config_path(None);
        let yaml = root.join("coflow.yaml");
        std::fs::write(&yaml, "schema: schema/main.cft\n").map_err(|err| err.to_string())?;
        let resolved = resolve_config_path(None).map_err(|err| err.to_string())?;

        let err = missing.expect_err("missing default config should fail");
        assert!(err.contains("no coflow.yaml or coflow.yml found"));
        assert_eq!(resolved, PathBuf::from(".").join("coflow.yaml"));
        Ok(())
    })();

    std::env::set_current_dir(&original_dir).map_err(|err| err.to_string())?;
    result?;
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_validation_reports_schema_source_and_output_edges() -> TestResult {
    let root = temp_project_dir("coflow-project-validation");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema/MAIN.CFT"),
        "type Upper { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(root.join("data.xlsx"), "").map_err(|err| err.to_string())?;

    let cases = [
        (
            "empty-schema-list",
            "schema: []\n",
            "schema list is empty",
            true,
        ),
        (
            "missing-schema",
            "schema: schema/missing.cft\n",
            "schema path `schema/missing.cft` does not exist",
            true,
        ),
        (
            "empty-schema-path",
            "schema: ''\n",
            "schema path is empty",
            true,
        ),
        (
            "uppercase-schema-file",
            "schema: schema/MAIN.CFT\n",
            "schema file `schema/MAIN.CFT` has unsupported extension",
            true,
        ),
        (
            "empty-source-path",
            "schema: schema/main.cft\nsources:\n  - path: ''\n    sheets:\n      - sheet: Items\n",
            "sources[0].path is empty",
            true,
        ),
        (
            "empty-source-url",
            "schema: schema/main.cft\nsources:\n  - url: '  '\n",
            "sources[0].url is empty",
            true,
        ),
        (
            "data-empty-dir",
            "schema: schema/main.cft\noutputs:\n  data:\n    type: json\n    dir: ''\n",
            "outputs.data.dir is empty",
            true,
        ),
        (
            "source-missing-path",
            "schema: schema/main.cft\nsources:\n  - path: missing.xlsx\n    sheets:\n      - sheet: Items\n",
            "sources[0].path `missing.xlsx` does not exist",
            false,
        ),
    ];

    for (name, yaml, expected, schema_only) in cases {
        let config = root.join(format!("{name}.yaml"));
        std::fs::write(&config, yaml).map_err(|err| err.to_string())?;
        let message = if schema_only {
            let project =
                Project::open_schema_only(Some(&config)).map_err(|err| err.to_string())?;
            project
                .schema_diagnostics()
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            Project::open(Some(&config)).expect_err("data validation should fail")
        };
        assert!(
            message.contains(expected),
            "case {name} expected `{expected}`, got `{message}`"
        );
    }

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_validation_returns_diagnostic_set_with_config_locations() -> TestResult {
    let root = temp_project_dir("coflow-project-diagnostic-set");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;
    let config = root.join("coflow.yaml");
    std::fs::write(&config, "schema: schema/main.cft\nsources:\n  - path: ''\n")
        .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&config)).map_err(|err| err.to_string())?;
    let diagnostics = project.schema_diagnostic_set();
    let expected_config = std::fs::canonicalize(&config).map_err(|err| err.to_string())?;

    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PROJECT-001"
                && diagnostic.stage == "PROJECT"
                && diagnostic.message == "sources[0].path is empty"
                && matches!(
                    diagnostic.primary.as_ref().map(|label| &label.location),
                    Some(SourceLocation::ProjectConfig { path, key_path })
                        if path == &expected_config && key_path.as_slice() == [
                            "sources".to_string(),
                            "0".to_string(),
                            "path".to_string(),
                        ]
                )),
        "diagnostics: {diagnostics:?}"
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_accepts_provider_neutral_source_and_output_types() -> TestResult {
    let root = temp_project_dir("coflow-project-provider-neutral-config");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(root.join("data.custom"), "").map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - type: custom-loader
    path: data.custom
    flavor: custom
outputs:
  data:
    type: custom-export
    dir: generated/custom-data
    compact: true
  code:
    type: custom-codegen
    dir: generated/custom-code
    namespace: Game.Custom
    runtime: unity
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let diagnostics = project.schema_diagnostics();
    assert!(
        diagnostics.is_empty(),
        "provider-neutral config should pass shape validation: {diagnostics:?}"
    );
    assert_eq!(
        project.config.sources[0].source_type.as_deref(),
        Some("custom-loader")
    );
    assert_eq!(
        project.config.sources[0].options["flavor"],
        serde_json::Value::String("custom".to_string())
    );
    assert!(project.config.sources[0].options.get("options").is_none());
    assert_eq!(
        project
            .config
            .outputs
            .data
            .as_ref()
            .expect("data output")
            .output_type,
        "custom-export"
    );
    assert_eq!(
        project
            .config
            .outputs
            .data
            .as_ref()
            .expect("data output")
            .options["compact"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        project
            .config
            .outputs
            .code
            .as_ref()
            .expect("code output")
            .output_type,
        "custom-codegen"
    );
    assert_eq!(
        project
            .config
            .outputs
            .code
            .as_ref()
            .expect("code output")
            .options["runtime"],
        serde_json::Value::String("unity".to_string())
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_accepts_new_path_url_sources_and_provider_options() -> TestResult {
    let root = temp_project_dir("coflow-project-new-config-model");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(root.join("data.custom"), "").map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - type: custom-loader
    path: data.custom
    flavor: custom
    sheets:
      - sheet: Item
        type: Item
  - type: lark-sheet
    url: https://example.feishu.cn/wiki/wiki_token
    app_id: ${COFLOW_TEST_APP_ID}
    app_secret: direct_secret
outputs:
  data:
    type: custom-export
    dir: generated/custom-data
    compact: true
  code:
    type: custom-codegen
    dir: generated/custom-code
    namespace: Game.Custom
"#,
    )
    .map_err(|err| err.to_string())?;
    std::env::set_var("COFLOW_TEST_APP_ID", "env_app");

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    assert!(project.schema_diagnostic_set().is_empty());
    assert_eq!(
        project.config.sources[0].source_type.as_deref(),
        Some("custom-loader")
    );
    assert_eq!(
        project.config.sources[0].location,
        coflow_project::SourceLocationSpec::Path(PathBuf::from("data.custom"))
    );
    assert_eq!(
        project.config.sources[0].options["flavor"],
        serde_json::Value::String("custom".to_string())
    );
    assert_eq!(
        project.config.sources[0].options["sheets"][0]["sheet"],
        serde_json::Value::String("Item".to_string())
    );
    assert_eq!(
        project.config.sources[1].location,
        coflow_project::SourceLocationSpec::Uri(
            "https://example.feishu.cn/wiki/wiki_token".to_string()
        )
    );
    assert_eq!(
        project.config.sources[1].options["app_id"],
        serde_json::Value::String("env_app".to_string())
    );
    assert_eq!(
        project
            .config
            .outputs
            .code
            .as_ref()
            .expect("code output")
            .options["namespace"],
        serde_json::Value::String("Game.Custom".to_string())
    );

    std::env::remove_var("COFLOW_TEST_APP_ID");
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_old_source_fields_and_missing_env_options() -> TestResult {
    let root = temp_project_dir("coflow-project-reject-old-config-model");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;

    for (name, yaml, expected) in [
        (
            "old-file",
            "schema: schema/main.cft\nsources:\n  - file: data.xlsx\n",
            "unknown field `file`",
        ),
        (
            "old-dir",
            "schema: schema/main.cft\nsources:\n  - dir: data\n",
            "unknown field `dir`",
        ),
        (
            "old-lark-sheet",
            "schema: schema/main.cft\nsources:\n  - lark_sheet:\n      app_id: a\n      app_secret: b\n      spreadsheet_token: t\n",
            "unknown field `lark_sheet`",
        ),
        (
            "missing-env",
            "schema: schema/main.cft\nsources:\n  - path: data.xlsx\n    token: ${COFLOW_MISSING_ENV_FOR_TEST}\n",
            "environment variable `COFLOW_MISSING_ENV_FOR_TEST` is not set",
        ),
    ] {
        let config = root.join(format!("{name}.yaml"));
        std::fs::write(&config, yaml).map_err(|err| err.to_string())?;
        let message = Project::open_schema_only(Some(&config))
            .err()
            .or_else(|| {
                Project::open_schema_only(Some(&config)).ok().map(|project| {
                    project
                        .schema_diagnostic_set()
                        .diagnostics
                        .into_iter()
                        .map(|diagnostic| diagnostic.message)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .unwrap_or_default();
        assert!(
            message.contains(expected),
            "case {name} expected `{expected}`, got `{message}`"
        );
    }

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_duplicate_provider_option_keys() -> TestResult {
    let root = temp_project_dir("coflow-project-duplicate-provider-options");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data.xlsx
    sheets:
      - sheet: Item
        columns:
          A: id
          A: name
"#,
    )
    .map_err(|err| err.to_string())?;

    let err = Project::open_schema_only(Some(&root)).expect_err("duplicate key should fail");

    assert!(
        err.contains("duplicate key `A`"),
        "expected duplicate key diagnostic, got `{err}`"
    );
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_accepts_remote_url_source_with_provider_sheet_options() -> TestResult {
    let root = temp_project_dir("coflow-project-remote-provider-sheet-options");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema/main.cft"),
        "type Item { name: string; }\n",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - type: lark-sheet
    url: https://fand3tbr90g.feishu.cn/wiki/K7M7wT1esizv6aklRy3cO4o6ntg
    app_id: cli_test
    app_secret: secret_test
    sheets:
      - sheet: 物品表
        type: Item
        key: 配置ID
        columns:
          名称: name
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    assert!(project.schema_diagnostic_set().is_empty());
    let source = &project.config.sources[0];
    assert_eq!(source.source_type.as_deref(), Some("lark-sheet"));
    assert_eq!(
        source.location,
        coflow_project::SourceLocationSpec::Uri(
            "https://fand3tbr90g.feishu.cn/wiki/K7M7wT1esizv6aklRy3cO4o6ntg".to_string()
        )
    );
    assert_eq!(source.options["app_id"], "cli_test");
    assert_eq!(source.options["app_secret"], "secret_test");
    assert_eq!(
        source.options["sheets"][0]["key"],
        serde_json::Value::String("配置ID".to_string())
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn validate_for_codegen_reports_unvalidated_output_combinations() -> TestResult {
    let root = temp_project_dir("coflow-project-codegen-validation");
    let missing_code = project_with_outputs(&root, OutputsConfig::default());
    let err = missing_code
        .validate_for_codegen()
        .expect_err("missing code output should fail");
    assert!(err.contains("missing outputs.code"));

    let wrong_code = project_with_outputs(
        &root,
        OutputsConfig {
            code: Some(output_config("java", "code", None)),
            data: Some(output_config("json", "data", None)),
        },
    );
    wrong_code
        .validate_for_codegen()
        .map_err(|err| format!("provider-neutral code output should validate: {err}"))?;

    let missing_data = project_with_outputs(
        &root,
        OutputsConfig {
            code: Some(output_config("csharp", "code", None)),
            data: None,
        },
    );
    let err = missing_data
        .validate_for_codegen()
        .expect_err("missing data output should fail");
    assert!(err.contains("missing outputs.data"));

    let wrong_data = project_with_outputs(
        &root,
        OutputsConfig {
            code: Some(output_config("csharp", "code", None)),
            data: Some(output_config("csv", "data", None)),
        },
    );
    wrong_data
        .validate_for_codegen()
        .map_err(|err| format!("provider-neutral data output should validate: {err}"))?;

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_recurses_only_cft_files_and_sorts_module_ids() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-files");
    std::fs::create_dir_all(root.join("schema/nested")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/z.cft"), "type Zed { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema/nested/a.cft"),
        "type Alpha { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/ignored.txt"), "type Ignored { }")
        .map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema\n").map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let module_ids = project
        .schema_files()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|file| file.module_id)
        .collect::<Vec<_>>();
    assert_eq!(module_ids, ["schema/nested/a.cft", "schema/z.cft"]);

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_ignores_uppercase_cft_extensions() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-files-extension-case");
    std::fs::create_dir_all(root.join("schema/nested")).map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema").join("MAIN.CFT"),
        "type Main { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema").join("nested").join("EXTRA.Cft"),
        "type Extra { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema\n").map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let schema_files = project.schema_files().map_err(|err| err.to_string())?;

    assert!(
        schema_files.is_empty(),
        "uppercase .CFT files should be ignored"
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_rejects_explicit_schema_file_with_non_lowercase_cft_extension() -> TestResult {
    let root = temp_project_dir("coflow-project-explicit-schema-extension-case");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema").join("MAIN.CFT"),
        "type Main { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/MAIN.CFT\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    let err = project
        .schema_files()
        .expect_err("uppercase explicit schema extension should fail");

    assert!(err.contains("schema file `schema/MAIN.CFT` has unsupported extension"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_accept_absolute_schema_paths_outside_project_root() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-files-absolute-root");
    let external = temp_project_dir("coflow-project-schema-files-absolute-external");
    let schema_path = external.join("external.cft");
    std::fs::write(&schema_path, "type External { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        format!("schema: {}\n", path_to_slash(&schema_path)),
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let files = project.schema_files().map_err(|err| err.to_string())?;

    assert_eq!(files.len(), 1);
    assert_eq!(normalize_path(&files[0].path), normalize_path(&schema_path));
    assert!(
        files[0].module_id.ends_with("external.cft"),
        "absolute paths outside the project should keep a usable module id, got `{}`",
        files[0].module_id
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())?;
    std::fs::remove_dir_all(external).map_err(|err| err.to_string())
}

#[test]
fn schema_overrides_match_by_module_or_path_and_reject_unmatched() -> TestResult {
    let root = temp_project_dir("coflow-project-overrides");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    let schema_path = root.join("schema/main.cft");
    std::fs::write(&schema_path, "type Item { value: string; }").map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    let by_module = SchemaSourceOverride {
        requested_module: Some("schema/main.cft".to_string()),
        normalized_path: normalize_path(&root.join("not-used.cft")),
        source: "type Replacement { value: string; }".to_string(),
    };
    let build = compile_schema_project_with_overrides(&project, &[by_module])
        .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("Replacement"));

    let by_path = SchemaSourceOverride {
        requested_module: None,
        normalized_path: normalize_path(&schema_path),
        source: "type PathReplacement { value: string; }".to_string(),
    };
    let build = compile_schema_project_with_overrides(&project, &[by_path])
        .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("PathReplacement"));

    let unmatched = SchemaSourceOverride {
        requested_module: Some("schema/missing.cft".to_string()),
        normalized_path: normalize_path(&root.join("schema/missing.cft")),
        source: "type Missing { value: string; }".to_string(),
    };
    let err = compile_schema_project_with_overrides(&project, &[unmatched])
        .expect_err("unmatched override should fail");
    assert!(err.contains("`--stdin-path schema/missing.cft` is not part"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_compile_with_invalid_module_keeps_diagnostics_without_compiling() -> TestResult {
    let root = temp_project_dir("coflow-project-invalid-module");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema/bad.cft"),
        "type Broken { value: Missing; }",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/bad.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    let build =
        compile_schema_project_with_overrides(&project, &[]).map_err(|err| err.to_string())?;

    assert!(build.container.is_none());
    assert!(build
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnknownNamedType));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_compile_with_override_parse_error_keeps_sources_and_paths() -> TestResult {
    let root = temp_project_dir("coflow-project-override-parse-error");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    let schema_path = root.join("schema/main.cft");
    std::fs::write(&schema_path, "type Item { value: string; }").map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

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

#[test]
fn diagnostic_json_uses_utf16_ranges_related_labels_and_dedup_keys() {
    let source = "type 表 {\n  名: string;\n}\n";
    let start = source.find('名').expect("field name");
    let end = start + "名".len();
    let diagnostic = CftDiagnostic::error(
        CftErrorCode::UnknownNamedType,
        ModuleId::new("schema/main.cft"),
        Span::new(start, end),
        "bad type",
    )
    .with_primary_message("primary")
    .with_related(
        ModuleId::new("schema/other.cft"),
        Span::new(0, 4),
        "related",
    );
    let duplicate = diagnostic.clone();
    let distinct = diagnostic.clone().with_related(
        ModuleId::new("schema/other.cft"),
        Span::new(5, 9),
        "related",
    );

    let deduped = dedupe_cft_diagnostics(vec![diagnostic.clone(), duplicate, distinct]);
    assert_eq!(deduped.len(), 2);

    let mut sources = BTreeMap::new();
    sources.insert("schema/main.cft".to_string(), source.to_string());
    sources.insert("schema/other.cft".to_string(), "enum E {}".to_string());
    let mut paths = BTreeMap::new();
    paths.insert(
        "schema/main.cft".to_string(),
        "C:/project/schema/main.cft".to_string(),
    );
    paths.insert(
        "schema/other.cft".to_string(),
        "C:/project/schema/other.cft".to_string(),
    );

    let json = DiagnosticJson::from_cft(&diagnostic, &sources, &paths);
    assert_eq!(json.path, "C:/project/schema/main.cft");
    assert_eq!(json.start_line, 1);
    assert_eq!(json.start_character, 2);
    assert_eq!(json.end_line, 1);
    assert_eq!(json.end_character, 3);
    assert_eq!(json.related.len(), 1);
    assert_eq!(json.related[0].path, "C:/project/schema/other.cft");
    assert_eq!(json.related[0].label.as_deref(), Some("related"));
}

#[test]
fn dedupe_cft_diagnostics_handles_diagnostics_without_primary_labels() {
    let diagnostic = CftDiagnostic {
        code: CftErrorCode::UnexpectedEof,
        stage: CftErrorCode::UnexpectedEof.stage(),
        severity: coflow_cft::CftSeverity::Error,
        message: "missing token".to_string(),
        primary: None,
        related: Vec::new(),
    };
    let duplicate = diagnostic.clone();

    let deduped = dedupe_cft_diagnostics(vec![diagnostic, duplicate]);

    assert_eq!(deduped.len(), 1);
    assert!(deduped[0].primary.is_none());
}

#[test]
fn path_helpers_normalize_nonexistent_paths_and_slash_components() {
    let path = Path::new("schema")
        .join("..")
        .join("other")
        .join(".")
        .join("file.cft");
    assert_eq!(
        normalize_path(&path),
        PathBuf::from("other").join("file.cft")
    );
    assert_eq!(
        path_to_slash(Path::new("schema").join("nested").join("a.cft").as_path()),
        "schema/nested/a.cft"
    );
}

fn project_with_outputs(root: &Path, outputs: OutputsConfig) -> Project {
    Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.to_path_buf(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema/main.cft")),
            sources: Vec::new(),
            outputs,
        },
    }
}

fn output_config(output_type: &str, dir: &str, namespace: Option<&str>) -> OutputConfig {
    let mut options = serde_json::Map::new();
    if let Some(namespace) = namespace {
        options.insert(
            "namespace".to_string(),
            serde_json::Value::String(namespace.to_string()),
        );
    }
    OutputConfig {
        output_type: output_type.to_string(),
        dir: PathBuf::from(dir),
        options: serde_json::Value::Object(options),
    }
}

#[test]
fn init_project_scaffolds_minimal_layout() -> TestResult {
    let dir = temp_project_dir("coflow-init");
    let outcome = init_project(&dir).map_err(|err| err.to_string())?;
    // Config landed at the expected path with the canonical template.
    assert_eq!(outcome.config_path, dir.join("coflow.yaml"));
    let written = std::fs::read_to_string(&outcome.config_path).expect("read config");
    assert_eq!(written, DEFAULT_PROJECT_YAML);
    // Standard subdirectories all exist.
    for sub in ["schema", "data", "generated/data", "generated/csharp"] {
        let p = dir.join(sub);
        if !p.is_dir() {
            return Err(format!("expected directory `{}`", p.display()));
        }
    }
    // The freshly scaffolded project must round-trip through the regular
    // open path — i.e. nothing about the layout is half-baked.
    let project = Project::open_schema_only(Some(outcome.config_path.as_path()))
        .map_err(|err| err.to_string())?;
    assert!(project.config.sources.is_empty());
    Ok(())
}

#[test]
fn init_project_refuses_to_clobber_existing_yaml() {
    let dir = temp_project_dir("coflow-init-existing");
    std::fs::write(dir.join("coflow.yaml"), "# existing\n").expect("seed yaml");
    let err = init_project(&dir).expect_err("must error");
    assert!(
        err.contains("already exists"),
        "expected clear refusal, got: {err}"
    );
    // The existing config wasn't touched.
    let preserved = std::fs::read_to_string(dir.join("coflow.yaml")).expect("read existing");
    assert_eq!(preserved, "# existing\n");
}

fn temp_project_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&root).expect("create temp dir");
    root
}
