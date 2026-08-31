use coflow_runtime::codegen::{CodeArtifactFile, CodeArtifactSet, CodegenError, CodegenRegistry};
use coflow_runtime::{
    MutationAppliedOp, MutationReport, Project, RecordCoordinate, Runtime, WriteOutcome,
};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_project() -> TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(
        dir.path().join("schema.cft"),
        "type Item { name: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data")).expect("data dir");
    fs::write(
        dir.path().join("data/items.cfd"),
        "Item { sword { name: \"Fire Sword\", } }\n",
    )
    .expect("data");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n",
    )
    .expect("config");
    dir
}

fn write_id_as_enum_project(is_flag: bool, records: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp enum project");
    let flag = if is_flag { "@flag " } else { "" };
    fs::write(
        dir.path().join("schema.cft"),
        format!("{flag}enum ItemId {{}}\n@idAsEnum(ItemId) type Item {{ name: string; }}\n"),
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data")).expect("data dir");
    fs::write(dir.path().join("data/items.cfd"), records).expect("data");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    )
    .expect("config");
    dir
}

fn active_enum_values(dir: &TempDir) -> serde_json::Map<String, Value> {
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".coflow/artifacts/active.json"))
            .expect("active manifest"),
    )
    .expect("valid active manifest");
    manifest["id_as_enum_values"]["ItemId"]
        .as_object()
        .expect("ItemId stable values")
        .clone()
}

#[test]
fn id_as_enum_values_are_stable_flag_safe_and_rename_aware() {
    let project_dir = write_id_as_enum_project(
        false,
        "beta: Item { name: \"Beta\" }\ngamma: Item { name: \"Gamma\" }\n",
    );
    let config = project_dir.path().join("coflow.yaml");
    let project = Project::open(Some(&config)).expect("open enum project");
    coflow::commands::generate_project_code(&project).expect("generate initial enum");
    assert_eq!(active_enum_values(&project_dir)["beta"], 0);
    assert_eq!(active_enum_values(&project_dir)["gamma"], 1);

    fs::write(
        project_dir.path().join("data/items.cfd"),
        "alpha: Item { name: \"Alpha\" }\nbeta: Item { name: \"Beta\" }\ngamma: Item { name: \"Gamma\" }\n",
    )
    .expect("insert sorted-first key");
    let project = Project::open(Some(&config)).expect("reopen enum project");
    coflow::commands::generate_project_code(&project).expect("regenerate stable enum");
    let values = active_enum_values(&project_dir);
    assert_eq!(values["beta"], 0);
    assert_eq!(values["gamma"], 1);
    assert_eq!(values["alpha"], 2);

    let session = Runtime::new()
        .open_write_session(project)
        .expect("open write session");
    let old = RecordCoordinate::try_new("Item", "beta").expect("old coordinate");
    let new = RecordCoordinate::try_new("Item", "delta").expect("new coordinate");
    let report = MutationReport {
        write_ok: true,
        check_ok: true,
        generation_changed: true,
        applied: vec![MutationAppliedOp {
            index: 0,
            op: "rename_record".to_string(),
            record: Some(old.clone()),
            file: Some("data/items.cfd".to_string()),
            outcome: WriteOutcome {
                renamed: Some((old, new)),
                ..Default::default()
            },
        }],
        failed: Vec::new(),
        affected_files: vec!["data/items.cfd".to_string()],
        diagnostics: Vec::new(),
    };
    assert!(
        coflow::commands::migrate_enum_lock_after_mutation(&session, &report)
            .expect("migrate renamed key")
    );
    let values = active_enum_values(&project_dir);
    assert_eq!(values["delta"], 0);
    assert!(!values.contains_key("beta"));

    let flag_dir = write_id_as_enum_project(
        true,
        "first: Item { name: \"First\" }\nsecond: Item { name: \"Second\" }\nthird: Item { name: \"Third\" }\n",
    );
    let flag_project =
        Project::open(Some(&flag_dir.path().join("coflow.yaml"))).expect("open flag enum project");
    coflow::commands::generate_project_code(&flag_project).expect("generate flag enum");
    let values = active_enum_values(&flag_dir);
    assert_eq!(values["first"], 1);
    assert_eq!(values["second"], 2);
    assert_eq!(values["third"], 4);
    let generated = fs::read_to_string(flag_dir.path().join("generated/csharp/ItemId.cs"))
        .expect("generated flag enum");
    assert!(generated.contains("None = 0"));
}

#[test]
fn project_uses_data_paths_and_direct_code_outputs() {
    let project = write_project();
    let opened =
        Project::open_schema_only(Some(&project.path().join("coflow.yaml"))).expect("open project");
    assert_eq!(opened.data_paths().len(), 1);
    assert_eq!(opened.data_paths()[0].path(), Path::new("data/"));
    assert_eq!(opened.config().codegen[0].language, "csharp");
}

#[test]
fn runtime_is_cfd_only_and_loads_the_project() {
    let project = write_project();
    fs::write(project.path().join("data/ignored.json"), "{}\n").expect("ignored file");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    let session = Runtime::new()
        .open_read_only_session(opened)
        .expect("load CFD");
    assert_eq!(session.queries().record_count_for_type("Item"), 1);
    assert!(session
        .queries()
        .source_files()
        .all(|path| path.ends_with(".cfd")));
}

#[test]
fn code_artifacts_reject_duplicate_or_traversal_paths() {
    let duplicate = CodeArtifactSet::new(vec![
        CodeArtifactFile {
            relative_path: "Item.cs".into(),
            contents: String::new(),
        },
        CodeArtifactFile {
            relative_path: "Item.cs".into(),
            contents: String::new(),
        },
    ]);
    assert!(matches!(
        duplicate,
        Err(CodegenError::DuplicateArtifactPath(_))
    ));
    let traversal = CodeArtifactSet::new(vec![CodeArtifactFile {
        relative_path: "../Item.cs".into(),
        contents: String::new(),
    }]);
    assert!(matches!(
        traversal,
        Err(CodegenError::InvalidArtifactPath(_))
    ));

    for paths in [
        vec!["Data/Item.cs", "data/item.cs"],
        vec!["generated", "generated/Item.cs"],
        vec!["AUX.cs"],
        vec!["generated/name?.cs"],
        vec!["generated/name. "],
    ] {
        let files = paths
            .into_iter()
            .map(|path| CodeArtifactFile {
                relative_path: path.into(),
                contents: String::new(),
            })
            .collect();
        assert!(CodeArtifactSet::new(files).is_err());
    }
}

#[test]
fn codegen_rejects_outputs_that_overlap_project_inputs() {
    let project = write_project();
    let marker = project.path().join("data/keep.txt");
    fs::write(&marker, "keep").expect("marker");

    for output in [".", "data/generated", ".coflow/artifacts/nested"] {
        fs::write(
            project.path().join("coflow.yaml"),
            format!(
                "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: {output}\n    namespace: Game.Config\n"
            ),
        )
        .expect("config");
        let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open");
        let error = coflow::commands::generate_project_code(&opened)
            .expect_err("overlapping output must be rejected");
        assert!(
            error.to_string().contains("overlap") || error.to_string().contains("project root")
        );
        assert_eq!(fs::read_to_string(&marker).expect("marker remains"), "keep");
    }
}

#[cfg(unix)]
#[test]
fn codegen_rejects_symlink_aliases_into_input_directories() {
    use std::os::unix::fs::symlink;

    let project = write_project();
    symlink(
        project.path().join("data"),
        project.path().join("data-alias"),
    )
    .expect("data alias");
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: data-alias/generated\n    namespace: Game.Config\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open");
    let error = coflow::commands::generate_project_code(&opened)
        .expect_err("symlinked overlapping output must be rejected");
    assert!(error.to_string().contains("overlaps data source"));
    assert!(!project.path().join("data/generated").exists());
}

#[test]
fn codegen_registry_is_separate_from_source_loading() {
    let mut codegen = CodegenRegistry::default();
    assert!(codegen
        .register(coflow_codegen_csharp::CsharpCfdCodeGenerator)
        .is_ok());
    assert!(codegen
        .register(coflow_codegen_csharp::CsharpCfdCodeGenerator)
        .is_err());
}

#[test]
fn cli_codegen_dispatches_through_the_language_registry() {
    let project = write_project();
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    let outcome = coflow::commands::generate_project_code(&opened).expect("generate code");
    let coflow::commands::CommandOutcome::Success(report) = outcome else {
        panic!("C# generator should be registered for the smoke project");
    };
    assert_eq!(report.targets.len(), 1);
    assert!(project
        .path()
        .join("generated/csharp/Coflow.Metadata.cs")
        .is_file());
}

#[test]
fn csharp_codegen_rejects_removed_numeric_width_options() {
    let project = write_project();
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n    int_32: true\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");

    let error = coflow::commands::generate_project_code(&opened)
        .expect_err("removed C# options must not be accepted");

    assert!(error.to_string().contains("unknown field `int_32`"));
    assert!(!project.path().join("generated/csharp").exists());
}

#[test]
fn codegen_failure_does_not_publish_an_earlier_target() {
    let project = write_project();
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/first\n    namespace: Game.Config\n  - language: not-installed\n    dir: generated/second\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    assert!(coflow::commands::generate_project_code(&opened).is_err());
    assert!(!project.path().join("generated/first").exists());
    assert!(!project.path().join("generated/second").exists());
}

#[test]
fn build_status_is_read_only_and_tracks_generated_contents() {
    let project_dir = write_project();
    let project =
        Project::open(Some(&project_dir.path().join("coflow.yaml"))).expect("open project");

    let status = coflow::commands::build_project_status(&project).expect("inspect build status");
    assert!(matches!(
        status,
        coflow::commands::CommandOutcome::Success(true)
    ));
    assert!(!project_dir.path().join("generated/csharp").exists());
    assert!(!project_dir.path().join(".coflow/artifacts").exists());

    coflow::commands::generate_project_code(&project).expect("generate code");
    let status = coflow::commands::build_project_status(&project).expect("inspect clean status");
    assert!(matches!(
        status,
        coflow::commands::CommandOutcome::Success(false)
    ));

    fs::write(
        project_dir
            .path()
            .join("generated/csharp/Coflow.Metadata.cs"),
        "changed",
    )
    .expect("change generated output");
    let status = coflow::commands::build_project_status(&project).expect("inspect changed status");
    assert!(matches!(
        status,
        coflow::commands::CommandOutcome::Success(true)
    ));
}

#[test]
fn codegen_reuses_history_preserves_unity_meta_and_cleans_inactive_generations() {
    let project_dir = write_project();
    let config = project_dir.path().join("coflow.yaml");
    let project = Project::open(Some(&config)).expect("open project");
    coflow::commands::generate_project_code(&project).expect("generate baseline");

    let generations = project_dir.path().join(".coflow/artifacts/generations");
    assert_eq!(fs::read_dir(&generations).expect("read history").count(), 1);
    fs::write(
        project_dir.path().join("generated/csharp/Item.cs.meta"),
        "unity-guid",
    )
    .expect("write Unity metadata");

    coflow::commands::generate_project_code(&project).expect("reuse unchanged output");
    assert_eq!(fs::read_dir(&generations).expect("read history").count(), 1);

    fs::write(
        project_dir.path().join("schema.cft"),
        "type Item { name: string; level: int = 1; }\n",
    )
    .expect("change schema");
    let changed = Project::open(Some(&config)).expect("reopen changed project");
    coflow::commands::generate_project_code(&changed).expect("generate changed output");
    assert_eq!(
        fs::read_to_string(project_dir.path().join("generated/csharp/Item.cs.meta"))
            .expect("read preserved Unity metadata"),
        "unity-guid"
    );
    assert_eq!(fs::read_dir(&generations).expect("read history").count(), 2);

    let cleaned = coflow::commands::clean_project(&changed).expect("clean history");
    assert_eq!(cleaned.generations_removed, 1);
    assert_eq!(fs::read_dir(&generations).expect("read history").count(), 1);
}

#[test]
fn csharp_codegen_emits_dimension_metadata_without_source_paths() {
    let dir = tempfile::tempdir().expect("dimension project");
    fs::write(
        dir.path().join("schema.cft"),
        "type UiText { @localized welcome: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data/dimensions/language")).expect("dimension dir");
    fs::write(
        dir.path().join("data/base.cfd"),
        "main: UiText { welcome: \"Hello\" }\n",
    )
    .expect("base CFD");
    fs::write(
        dir.path()
            .join("data/dimensions/language/UiText_welcome.cfd"),
        "main: UiText { default: \"Hello\", zh: \"你好\" }\n",
    )
    .expect("dimension CFD");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n",
    )
    .expect("config");

    let project = Project::open(Some(&dir.path().join("coflow.yaml"))).expect("open project");
    let outcome = coflow::commands::generate_project_code(&project).expect("generate C#");
    assert!(matches!(
        outcome,
        coflow::commands::CommandOutcome::Success(_)
    ));
    let generated = fs::read_to_string(dir.path().join("generated/csharp/Coflow.Metadata.cs"))
        .expect("generated CFD binding");
    assert!(!generated.contains("data/dimensions/language/UiText_welcome.cfd"));
    assert!(generated.contains("ReadLanguage("));
    assert!(generated.contains("context.FindRecord(variantsType, recordKey)"));
    assert!(!generated.contains("Localization"));
    assert!(!generated.contains("TbUiTextWelcomeVariants"));
}

#[test]
fn csharp_codegen_reads_each_singleton_dimension_record() {
    let dir = tempfile::tempdir().expect("singleton dimension project");
    fs::write(
        dir.path().join("schema.cft"),
        "@singleton type UiText { @localized welcome: string; @localized farewell: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data/dimensions/language")).expect("dimension dir");
    fs::write(
        dir.path().join("data/base.cfd"),
        "UiText: UiText { welcome: \"Hello\", farewell: \"Bye\" }\n",
    )
    .expect("base CFD");
    fs::write(
        dir.path().join("data/dimensions/language/UiText.cfd"),
        "welcome: UiText { zh: \"你好\" }\nfarewell: UiText { zh: \"再见\" }\n",
    )
    .expect("dimension CFD");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    )
    .expect("config");

    let project = Project::open(Some(&dir.path().join("coflow.yaml"))).expect("open project");
    let outcome = coflow::commands::generate_project_code(&project).expect("generate C#");
    assert!(matches!(
        outcome,
        coflow::commands::CommandOutcome::Success(_)
    ));
    let generated = fs::read_to_string(dir.path().join("generated/csharp/Coflow.Metadata.cs"))
        .expect("generated CFD binding");
    assert!(!generated.contains("data/dimensions/language/UiText.cfd"));
    assert!(generated.contains("\"UiText_welcomeVariants\", \"welcome\", new string[] { \"zh\" }"));
    assert!(
        generated.contains("\"UiText_farewellVariants\", \"farewell\", new string[] { \"zh\" }")
    );
}

#[test]
fn rust_runtime_rejects_dimension_record_type_mismatches_like_csharp() {
    let dir = tempfile::tempdir().expect("dimension project");
    fs::write(
        dir.path().join("schema.cft"),
        "type UiText { @localized welcome: string; } type Other { value: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data/dimensions/language")).expect("dimension dir");
    fs::write(
        dir.path().join("data/base.cfd"),
        "main: UiText { welcome: \"Hello\" }\n",
    )
    .expect("base CFD");
    fs::write(
        dir.path()
            .join("data/dimensions/language/UiText_welcome.cfd"),
        "main: Other { zh: \"错误\" }\n",
    )
    .expect("invalid dimension CFD");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    )
    .expect("config");

    let project = Project::open(Some(&dir.path().join("coflow.yaml"))).expect("open project");
    let diagnostics = match Runtime::new().open_read_only_session(project) {
        Ok(session) => session.into_diagnostics(),
        Err(diagnostics) => diagnostics,
    };
    let diagnostic = diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "CFD-DIMENSION-TYPE")
        .expect("dimension type diagnostic");
    let primary = diagnostic
        .primary
        .as_ref()
        .expect("dimension type source span");
    let coflow_runtime::SourceLocation::FileSpan {
        path,
        start_line,
        start_character,
        end_line,
        end_character,
    } = &primary.location
    else {
        panic!("dimension type diagnostic should point into the overlay CFD");
    };
    let expected_path = fs::canonicalize(
        dir.path()
            .join("data/dimensions/language/UiText_welcome.cfd"),
    )
    .expect("canonical dimension CFD path");
    assert_eq!(path, &expected_path);
    assert_eq!((*start_line, *start_character), (0, 6));
    assert_eq!((*end_line, *end_character), (0, 11));
}

#[test]
fn rust_runtime_rejects_unknown_singleton_dimension_rows_and_variants() {
    let dir = tempfile::tempdir().expect("dimension project");
    fs::write(
        dir.path().join("schema.cft"),
        "@singleton type UiText { @localized welcome: string; @localized farewell: string; }\n",
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data/dimensions/language")).expect("dimension dir");
    fs::write(
        dir.path().join("data/base.cfd"),
        "ui: UiText { welcome: \"Hello\", farewell: \"Bye\" }\n",
    )
    .expect("base CFD");
    fs::write(
        dir.path().join("data/dimensions/language/UiText.cfd"),
        "welcome: UiText { zh: \"你好\", typo_variant: \"错误\" }\nunknown: UiText { zh: \"错误\" }\n",
    )
    .expect("invalid dimension CFD");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/base.cfd\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    )
    .expect("config");

    let project = Project::open(Some(&dir.path().join("coflow.yaml"))).expect("open project");
    let diagnostics = match Runtime::new().open_read_only_session(project) {
        Ok(session) => session.into_diagnostics(),
        Err(diagnostics) => diagnostics,
    };
    for code in ["CFD-DIMENSION-FIELD", "CFD-DIMENSION-VARIANT"] {
        let diagnostic = diagnostics
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap_or_else(|| panic!("missing {code}"));
        assert!(matches!(
            diagnostic.primary.as_ref().map(|label| &label.location),
            Some(coflow_runtime::SourceLocation::FileSpan { path, .. })
                if path.ends_with("data/dimensions/language/UiText.cfd")
        ));
    }
}
