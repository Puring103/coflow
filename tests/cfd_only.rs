use coflow_codegen_api::{CodeArtifactFile, CodeArtifactSet, CodegenError, CodegenRegistry};
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
    let opened = Project::open_schema_only(Some(&project.path().join("coflow.yaml")))
        .expect("open project");
    assert_eq!(opened.data_paths().len(), 1);
    assert_eq!(opened.data_paths()[0].path(), Path::new("data/"));
    assert_eq!(opened.config().codegen[0].language, "csharp");
}

#[test]
fn runtime_is_cfd_only_and_loads_the_project() {
    let project = write_project();
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    let session = Runtime::new()
        .open_read_only_session(opened)
        .expect("load CFD");
    assert_eq!(session.queries().record_count_for_type("Item"), 1);
    assert!(session.queries().source_files().all(|path| path.ends_with(".cfd")));
}

#[test]
fn code_artifacts_reject_duplicate_or_traversal_paths() {
    let duplicate = CodeArtifactSet::new(vec![
        CodeArtifactFile { relative_path: "Item.cs".into(), contents: String::new() },
        CodeArtifactFile { relative_path: "Item.cs".into(), contents: String::new() },
    ]);
    assert!(matches!(duplicate, Err(CodegenError::DuplicateArtifactPath(_))));
    let traversal = CodeArtifactSet::new(vec![CodeArtifactFile {
        relative_path: "../Item.cs".into(),
        contents: String::new(),
    }]);
    assert!(matches!(traversal, Err(CodegenError::InvalidArtifactPath(_))));
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
