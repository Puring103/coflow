use coflow_cft::{CftDiagnostic, CftErrorCode, ModuleId, Span};
use coflow_project::{
    compile_schema_project_with_overrides, dedupe_cft_diagnostics, normalize_path, path_to_slash,
    resolve_config_path, DiagnosticJson, OutputConfig, OutputsConfig, Project, ProjectConfig,
    SchemaConfig, SchemaSourceOverride,
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
    std::fs::write(
        root.join("schema/main.cft"),
        "type Item { @id id: string; }",
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
            "empty-source-file",
            "schema: schema/main.cft\nsources:\n  - file: ''\n    sheets:\n      - sheet: Items\n",
            "sources[0].file is empty",
            true,
        ),
        (
            "empty-sheet",
            "schema: schema/main.cft\nsources:\n  - file: data.xlsx\n    sheets:\n      - sheet: '  '\n",
            "sources[0].sheets[0].sheet is empty",
            true,
        ),
        (
            "empty-sheet-type",
            "schema: schema/main.cft\nsources:\n  - file: data.xlsx\n    sheets:\n      - sheet: Items\n        type: ' '\n",
            "sources[0].sheets[0].type is empty",
            true,
        ),
        (
            "data-namespace",
            "schema: schema/main.cft\noutputs:\n  data:\n    type: json\n    dir: out\n    namespace: Bad\n",
            "outputs.data.namespace is only valid for code outputs",
            true,
        ),
        (
            "data-empty-dir",
            "schema: schema/main.cft\noutputs:\n  data:\n    type: json\n    dir: ''\n",
            "outputs.data.dir is empty",
            true,
        ),
        (
            "data-unsupported-type",
            "schema: schema/main.cft\noutputs:\n  data:\n    type: xml\n    dir: out\n",
            "outputs.data.type is `xml`; expected `json` or `messagepack`",
            true,
        ),
        (
            "code-unsupported-type",
            "schema: schema/main.cft\noutputs:\n  code:\n    type: java\n    dir: out\n",
            "outputs.code.type is `java`; expected `csharp`",
            true,
        ),
        (
            "code-empty-namespace",
            "schema: schema/main.cft\noutputs:\n  code:\n    type: csharp\n    dir: out\n    namespace: ' '\n",
            "outputs.code.namespace is empty",
            true,
        ),
        (
            "source-missing-file",
            "schema: schema/main.cft\nsources:\n  - file: missing.xlsx\n    sheets:\n      - sheet: Items\n",
            "sources[0].file `missing.xlsx` does not exist",
            false,
        ),
        (
            "source-empty-sheets",
            "schema: schema/main.cft\nsources:\n  - file: data.xlsx\n    sheets: []\n",
            "sources[0].sheets is empty",
            false,
        ),
    ];

    for (name, yaml, expected, schema_only) in cases {
        let config = root.join(format!("{name}.yaml"));
        std::fs::write(&config, yaml).map_err(|err| err.to_string())?;
        let err = if schema_only {
            Project::open_schema_only(Some(&config))
                .expect_err("schema-only validation should fail")
        } else {
            Project::open(Some(&config)).expect_err("data validation should fail")
        };
        assert!(
            err.contains(expected),
            "case {name} expected `{expected}`, got `{err}`"
        );
    }

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
    let err = wrong_code
        .validate_for_codegen()
        .expect_err("wrong code output type should fail");
    assert!(err.contains("outputs.code.type is `java`; expected `csharp`"));

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
    let err = wrong_data
        .validate_for_codegen()
        .expect_err("wrong data output type should fail");
    assert!(err.contains("outputs.data.type is `csv`; expected `json` or `messagepack`"));

    std::fs::remove_dir_all(root).map_err(|err| err.to_string())
}

#[test]
fn schema_files_recurses_only_cft_files_and_sorts_module_ids() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-files");
    std::fs::create_dir_all(root.join("schema/nested")).map_err(|err| err.to_string())?;
    std::fs::write(root.join("schema/z.cft"), "type Zed { @id id: string; }")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        root.join("schema/nested/a.cft"),
        "type Alpha { @id id: string; }",
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
fn schema_files_accept_absolute_schema_paths_outside_project_root() -> TestResult {
    let root = temp_project_dir("coflow-project-schema-files-absolute-root");
    let external = temp_project_dir("coflow-project-schema-files-absolute-external");
    let schema_path = external.join("external.cft");
    std::fs::write(&schema_path, "type External { @id id: string; }")
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
    std::fs::write(&schema_path, "type Item { @id id: string; }").map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    let by_module = SchemaSourceOverride {
        requested_module: Some("schema/main.cft".to_string()),
        normalized_path: normalize_path(&root.join("not-used.cft")),
        source: "type Replacement { @id id: string; }".to_string(),
    };
    let build = compile_schema_project_with_overrides(&project, &[by_module])
        .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("Replacement"));

    let by_path = SchemaSourceOverride {
        requested_module: None,
        normalized_path: normalize_path(&schema_path),
        source: "type PathReplacement { @id id: string; }".to_string(),
    };
    let build = compile_schema_project_with_overrides(&project, &[by_path])
        .map_err(|err| err.to_string())?;
    assert!(build.container.is_some());
    assert!(build.sources["schema/main.cft"].contains("PathReplacement"));

    let unmatched = SchemaSourceOverride {
        requested_module: Some("schema/missing.cft".to_string()),
        normalized_path: normalize_path(&root.join("schema/missing.cft")),
        source: "type Missing { @id id: string; }".to_string(),
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
    std::fs::write(&schema_path, "type Item { @id id: string; }").map_err(|err| err.to_string())?;
    std::fs::write(root.join("coflow.yaml"), "schema: schema/main.cft\n")
        .map_err(|err| err.to_string())?;
    let project = Project::open_schema_only(Some(&root)).map_err(|err| err.to_string())?;

    let override_source = "type Broken { @id id: string;".to_string();
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
    OutputConfig {
        output_type: output_type.to_string(),
        dir: PathBuf::from(dir),
        namespace: namespace.map(str::to_string),
    }
}

fn temp_project_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&root).expect("create temp dir");
    root
}
