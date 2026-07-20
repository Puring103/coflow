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

use coflow_api::{path_to_slash as canonical_path_to_slash, SourceLocation};
use coflow_project::{
    discover_directory_files, init_project, normalize_path, normalized_path_identity,
    path_is_same_or_descendant, path_to_slash, resolve_config_path, OutputConfig, OutputsConfig,
    Project, ProjectConfig, SchemaConfig, DEFAULT_PROJECT_YAML,
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
            "remote-source-url",
            "schema: schema/main.cft\nsources:\n  - url: '  '\n",
            "unknown field `url`",
            false,
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
                .schema_diagnostic_set()
                .diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            Project::open(Some(&config))
                .expect_err("data validation should fail")
                .to_string()
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
    let diagnostics = project.schema_diagnostic_set();
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
fn project_config_accepts_path_sources_and_provider_options() -> TestResult {
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
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    assert!(project.schema_diagnostic_set().is_empty());
    assert_eq!(
        project.config.sources[0].source_type.as_deref(),
        Some("custom-loader")
    );
    assert_eq!(
        project.config.sources[0].location,
        coflow_api::SourceLocationSpec::new(PathBuf::from("data.custom"))
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
        project
            .config
            .outputs
            .code
            .as_ref()
            .expect("code output")
            .options["namespace"],
        serde_json::Value::String("Game.Custom".to_string())
    );

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_old_source_fields() -> TestResult {
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
    ] {
        let config = root.join(format!("{name}.yaml"));
        std::fs::write(&config, yaml).map_err(|err| err.to_string())?;
        let message = Project::open_schema_only(Some(&config))
            .err()
            .map(|diagnostics| diagnostics.to_string())
            .or_else(|| {
                Project::open_schema_only(Some(&config))
                    .ok()
                    .map(|project| {
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
fn project_config_preserves_env_like_strings() -> TestResult {
    let root = temp_project_dir("coflow-project-preserve-env-like-strings");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data.xlsx
    token: ${COFLOW_LITERAL_TOKEN}
    nested:
      app_id: ${COFLOW_NESTED_APP_ID}
      values:
        - ${COFLOW_ARRAY_TOKEN}
outputs:
  data:
    type: json
    dir: generated/data
    token: ${COFLOW_OUTPUT_TOKEN}
"#,
    )
    .map_err(|err| err.to_string())?;
    std::env::set_var("COFLOW_LITERAL_TOKEN", "expanded-token");
    std::env::set_var("COFLOW_NESTED_APP_ID", "expanded-app");
    std::env::set_var("COFLOW_ARRAY_TOKEN", "expanded-array");
    std::env::set_var("COFLOW_OUTPUT_TOKEN", "expanded-output");

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    assert_eq!(
        project.config.sources[0].options["token"],
        "${COFLOW_LITERAL_TOKEN}"
    );
    assert_eq!(
        project.config.sources[0].options["nested"]["app_id"],
        "${COFLOW_NESTED_APP_ID}"
    );
    assert_eq!(
        project.config.sources[0].options["nested"]["values"][0],
        "${COFLOW_ARRAY_TOKEN}"
    );
    assert_eq!(
        project
            .config
            .outputs
            .data
            .as_ref()
            .expect("data output")
            .options["token"],
        "${COFLOW_OUTPUT_TOKEN}"
    );

    std::env::remove_var("COFLOW_LITERAL_TOKEN");
    std::env::remove_var("COFLOW_NESTED_APP_ID");
    std::env::remove_var("COFLOW_ARRAY_TOKEN");
    std::env::remove_var("COFLOW_OUTPUT_TOKEN");
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
fn project_config_accepts_language_dimension_config() -> TestResult {
    let root = temp_project_dir("coflow-project-dimensions-config");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let language = project
        .config
        .dimensions
        .get("language")
        .expect("language dimension");
    assert_eq!(language.variants, ["zh".to_string(), "en".to_string()]);
    assert_eq!(
        language.out_dir.as_deref(),
        Some(Path::new("data/dimensions/language"))
    );
    assert!(project.schema_diagnostic_set().is_empty());

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_removed_localization_key() -> TestResult {
    let root = temp_project_dir("coflow-project-localization-removed");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    let config = root.join("coflow.yaml");
    std::fs::write(
        &config,
        r#"schema: schema/main.cft
localization:
  languages: [zh]
"#,
    )
    .map_err(|err| err.to_string())?;

    let err =
        Project::open_schema_only(Some(&config)).expect_err("old localization key should fail");
    assert!(err.contains("PROJECT-CONFIG-LOCALIZATION-REMOVED"));
    assert!(err.contains("`localization` has been removed; use `dimensions.language` instead"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_validates_language_dimension_variants_and_out_dir() -> TestResult {
    let root = temp_project_dir("coflow-project-dimensions-validation");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;

    let cases = [
        (
            "missing-out-dir",
            r#"schema: schema/main.cft
dimensions:
  language:
    variants: [zh]
"#,
            "DIM-CONFIG-003",
            "dimensions.language.out_dir is required",
        ),
        (
            "empty-variants",
            r#"schema: schema/main.cft
dimensions:
  language:
    variants: []
    out_dir: data/dimensions/language
"#,
            "DIM-CONFIG-002",
            "dimensions.language.variants must not be empty",
        ),
        (
            "reserved-default",
            r#"schema: schema/main.cft
dimensions:
  language:
    variants: [default]
    out_dir: data/dimensions/language
"#,
            "DIM-CONFIG-002",
            "dimensions.language.variants cannot include reserved variant `default`",
        ),
        (
            "invalid-ident",
            r#"schema: schema/main.cft
dimensions:
  language:
    variants: [zh-CN]
    out_dir: data/dimensions/language
"#,
            "DIM-CONFIG-002",
            "dimensions.language.variants[0] `zh-CN` is not a valid CFT identifier",
        ),
        (
            "duplicate",
            r#"schema: schema/main.cft
dimensions:
  language:
    variants: [zh, zh]
    out_dir: data/dimensions/language
"#,
            "DIM-CONFIG-002",
            "dimensions.language.variants contains duplicate variant `zh`",
        ),
    ];

    for (name, yaml, code, expected) in cases {
        let config = root.join(format!("{name}.yaml"));
        std::fs::write(&config, yaml).map_err(|err| err.to_string())?;
        let project = Project::open_schema_only(Some(&config)).map_err(|err| err.to_string())?;
        let diagnostics = project.schema_diagnostic_set();
        assert!(
            diagnostics
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.message == expected),
            "case {name} expected `{code}: {expected}`, got {diagnostics:?}"
        );
    }

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_validates_every_dimension_without_language_special_cases() -> TestResult {
    let root = temp_project_dir("coflow-project-custom-dimension-validation");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
dimensions:
  platform:
    variants: []
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let diagnostics = project.schema_diagnostic_set();
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DIM-CONFIG-002"
            && diagnostic.message == "dimensions.platform.variants must not be empty"
    }));
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DIM-CONFIG-003"
            && diagnostic.message == "dimensions.platform.out_dir is required"
    }));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_overlapping_dimension_output_directories() -> TestResult {
    let root = temp_project_dir("coflow-project-dimension-directory-ownership");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions
  platform:
    variants: [pc]
    out_dir: data/dimensions/platform
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let diagnostics = project.schema_diagnostic_set();
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DIM-SOURCE-007"
            && diagnostic
                .message
                .contains("every dimension requires an exclusive managed directory")
    }));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn project_config_rejects_sources_inside_dimension_out_dir() -> TestResult {
    let root = temp_project_dir("coflow-project-dimension-source-overlap");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::create_dir_all(root.join("data/dimensions/language"))
        .map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Item { name: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh\n",
    )
    .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/main.cft
sources:
  - path: data/dimensions/language/Item_name.csv
dimensions:
  language:
    variants: [zh]
    out_dir: data/dimensions/language
"#,
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let diagnostics = project.schema_diagnostic_set();
    assert!(
        diagnostics.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "DIM-SOURCE-003"
                && diagnostic.message.contains("is managed by Coflow")
        }),
        "expected dimension source overlap diagnostic, got {diagnostics:?}"
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
fn schema_files_deduplicate_canonical_file_identities() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-file-identities");
    std::fs::create_dir_all(root.join("schema")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/main.cft"), "type Main { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("coflow.yaml"),
        "schema:\n  - schema\n  - schema/../schema/main.cft\n",
    )
    .map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let files = project.schema_files().map_err(|err| err.to_string())?;

    assert_eq!(files.len(), 1, "the same canonical file must load once");
    assert_eq!(files[0].module_id, "schema/main.cft");

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_terminate_directory_alias_cycles_and_deduplicate() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-directory-cycle");
    let schema_dir = root.join("schema");
    let nested_dir = schema_dir.join("nested");
    std::fs::create_dir_all(&nested_dir).map_err(|err| err.to_string())?;
    std::fs::write(schema_dir.join("main.cft"), "type Main { value: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        nested_dir.join("extra.cft"),
        "type Extra { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    let alias = nested_dir.join("back_to_schema");
    create_directory_alias(&alias, &schema_dir);
    std::fs::write(root.join("coflow.yaml"), "schema: schema\n").map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let files = project.schema_files().map_err(|err| err.to_string())?;
    let module_ids = files
        .iter()
        .map(|file| file.module_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(module_ids, ["schema/main.cft", "schema/nested/extra.cft"]);
    remove_directory_alias(&alias).map_err(|err| err.to_string())?;
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_reject_directory_aliases_outside_declared_root() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-directory-outside");
    let external = temp_project_dir("coflow-project-schema-directory-external");
    let schema_dir = root.join("schema");
    std::fs::create_dir_all(&schema_dir).map_err(|err| err.to_string())?;
    std::fs::write(
        external.join("outside.cft"),
        "type Outside { value: string; }",
    )
    .map_err(|err| err.to_string())?;
    let alias = schema_dir.join("outside");
    create_directory_alias(&alias, &external);
    std::fs::write(root.join("coflow.yaml"), "schema: schema\n").map_err(|err| err.to_string())?;

    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;
    let diagnostics = project
        .schema_files()
        .expect_err("alias outside the declared schema root must fail");
    assert!(diagnostics.contains("resolves outside declared root"));

    remove_directory_alias(&alias).map_err(|err| err.to_string())?;
    std::fs::remove_dir_all(root).map_err(|err| err.to_string())?;
    std::fs::remove_dir_all(external).map_err(|err| err.to_string())
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
    assert!(path_is_same_or_descendant(
        Path::new("data/dimensions/language/Item_name.csv"),
        Path::new("data/dimensions/language")
    ));
    assert!(!path_is_same_or_descendant(
        Path::new("data/dimensions/platform"),
        Path::new("data/dimensions/language")
    ));
    if cfg!(windows) {
        assert_eq!(
            normalized_path_identity(Path::new("DATA/Dimensions")),
            normalized_path_identity(Path::new("data/dimensions"))
        );
    }
    let project_formatter: fn(&Path) -> String = path_to_slash;
    let canonical_formatter: fn(&Path) -> String = canonical_path_to_slash;
    assert!(std::ptr::fn_addr_eq(project_formatter, canonical_formatter));
}

fn project_with_outputs(root: &Path, outputs: OutputsConfig) -> Project {
    Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.to_path_buf(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema/main.cft")),
            sources: Vec::new(),
            outputs,
            dimensions: BTreeMap::new(),
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

#[test]
fn init_project_removes_new_paths_after_staging_failure() {
    let dir = temp_project_dir("coflow-init-rollback");
    std::fs::create_dir_all(dir.join("generated")).expect("create existing generated directory");
    std::fs::write(dir.join("generated/data"), "conflict").expect("seed path conflict");

    let err = init_project(&dir).expect_err("staging conflict must fail");

    assert!(err.contains("because it is a file"));
    assert!(!dir.join("coflow.yaml").exists());
    assert!(!dir.join("schema").exists());
    assert!(!dir.join("data").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("generated/data")).expect("preserved conflict"),
        "conflict"
    );
    std::fs::remove_dir_all(dir).expect("clean rollback project");
}

#[test]
fn concurrent_project_initialization_has_one_winner() {
    let dir = temp_project_dir("coflow-init-concurrent");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let dir = dir.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            init_project(dir)
        }));
    }
    barrier.wait();
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("initializer thread"))
        .collect::<Vec<_>>();

    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("coflow.yaml")).expect("published config"),
        DEFAULT_PROJECT_YAML
    );
    std::fs::remove_dir_all(dir).expect("clean concurrent project");
}

#[test]
fn directory_discovery_visits_canonical_directories_once() {
    let root = temp_project_dir("coflow-directory-discovery-cycle");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).expect("create nested directory");
    let source = root.join("items.cfd");
    std::fs::write(&source, "item: Item {}\n").expect("write source");
    let alias = nested.join("root-alias");
    create_directory_alias(&alias, &root);

    let files = discover_directory_files(&root).expect("discover cyclic source tree");

    assert_eq!(files, vec![source]);
    remove_directory_alias(&alias).expect("remove directory alias");
    std::fs::remove_dir_all(root).expect("clean source tree");
}

#[test]
fn directory_discovery_rejects_targets_outside_declared_root() {
    let root = temp_project_dir("coflow-directory-discovery-root");
    let external = temp_project_dir("coflow-directory-discovery-external");
    std::fs::write(external.join("outside.cfd"), "item: Item {}\n").expect("write external source");
    let alias = root.join("outside-alias");
    create_directory_alias(&alias, &external);

    let error = discover_directory_files(&root).expect_err("reject outside directory alias");

    assert_eq!(error.path(), alias);
    assert!(error.to_string().contains("resolves outside declared root"));
    remove_directory_alias(&alias).expect("remove directory alias");
    std::fs::remove_dir_all(root).expect("clean source root");
    std::fs::remove_dir_all(external).expect("clean external root");
}

#[cfg(unix)]
fn create_directory_alias(alias: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, alias).expect("create directory symlink");
}

#[cfg(windows)]
fn create_directory_alias(alias: &Path, target: &Path) {
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(alias)
        .arg(target)
        .output()
        .expect("create directory junction");
    assert!(
        output.status.success(),
        "failed to create junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn remove_directory_alias(alias: &Path) -> std::io::Result<()> {
    std::fs::remove_file(alias)
}

#[cfg(windows)]
fn remove_directory_alias(alias: &Path) -> std::io::Result<()> {
    std::fs::remove_dir(alias)
}

fn temp_project_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&root).expect("create temp dir");
    root
}
