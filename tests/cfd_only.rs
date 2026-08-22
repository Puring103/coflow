use coflow_runtime::codegen::{CodeArtifactFile, CodeArtifactSet, CodegenError, CodegenRegistry};
use coflow_runtime::Project;
use coflow_runtime::Runtime;
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
        .join("generated/csharp/CoflowTables.Cfd.cs")
        .is_file());
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
fn csharp_codegen_normalizes_and_loads_dimension_cfd_sources() {
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
    assert!(matches!(outcome, coflow::commands::CommandOutcome::Success(_)));
    let generated = fs::read_to_string(
        dir.path().join("generated/csharp/CoflowTables.Cfd.cs"),
    )
    .expect("generated CFD binding");
    assert_eq!(
        generated
            .matches("data/dimensions/language/UiText_welcome.cfd")
            .count(),
        2,
        "dimension source appears once in SourceFiles and once in its normalizer"
    );
    assert!(generated.contains(
        "NormalizeDimensionRecord(record, \"UiText\", \"UiText_welcomeVariants\", document.Path)"
    ));
    assert!(generated.contains("ReadLocalized("));
    assert!(generated.contains("context.FindRecord(variantsType, recordKey)"));
    assert!(!generated.contains("LocalizationProvider"));
    assert!(!generated.contains("TbUiTextWelcomeVariants"));
}

#[test]
fn csharp_codegen_dispatches_singleton_dimension_rows_by_field_key() {
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
    assert!(matches!(outcome, coflow::commands::CommandOutcome::Success(_)));
    let generated = fs::read_to_string(
        dir.path().join("generated/csharp/CoflowTables.Cfd.cs"),
    )
    .expect("generated CFD binding");
    assert_eq!(
        generated
            .matches("data/dimensions/language/UiText.cfd")
            .count(),
        2,
        "shared singleton source is loaded once and normalized once"
    );
    assert!(generated.contains(
        "\"welcome\" => NormalizeDimensionRecord(record, \"UiText\", \"UiText_welcomeVariants\", document.Path)"
    ));
    assert!(generated.contains(
        "\"farewell\" => NormalizeDimensionRecord(record, \"UiText\", \"UiText_farewellVariants\", document.Path)"
    ));
    assert!(generated.contains(
        "\"UiText_welcomeVariants\", \"welcome\", new string[] { \"zh\" }"
    ));
    assert!(generated.contains(
        "\"UiText_farewellVariants\", \"farewell\", new string[] { \"zh\" }"
    ));
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
    let primary = diagnostic.primary.as_ref().expect("dimension type source span");
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
    assert_eq!(
        path,
        &dir.path()
            .join("data/dimensions/language/UiText_welcome.cfd")
    );
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
