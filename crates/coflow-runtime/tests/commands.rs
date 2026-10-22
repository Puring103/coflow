#![allow(clippy::expect_used, clippy::panic)]

use coflow_runtime::{MutationOp, MutationRequest, Project, RecordCoordinate, Runtime};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_project() -> TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; }\n",
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
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
    )
    .expect("config");
    dir
}

fn write_id_as_enum_project(is_flag: bool, records: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp enum project");
    let flag = if is_flag { "@flag " } else { "" };
    fs::write(
        dir.path().join("schema.cft"),
        format!("{flag}enum ItemId {{}}\n@idAsEnum(ItemId) table Item {{ name: string; }}\n"),
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
    let lock: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join("coflow.enum.lock.json")).expect("enum lockfile"),
    )
    .expect("valid enum lockfile");
    lock["ItemId"]
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
    coflow_runtime::commands::generate_project_code(&project).expect("generate initial enum");
    assert_eq!(active_enum_values(&project_dir)["beta"], 0);
    assert_eq!(active_enum_values(&project_dir)["gamma"], 1);

    fs::write(
        project_dir.path().join("data/items.cfd"),
        "alpha: Item { name: \"Alpha\" }\nbeta: Item { name: \"Beta\" }\ngamma: Item { name: \"Gamma\" }\n",
    )
    .expect("insert sorted-first key");
    let project = Project::open(Some(&config)).expect("reopen enum project");
    coflow_runtime::commands::generate_project_code(&project).expect("regenerate stable enum");
    let values = active_enum_values(&project_dir);
    assert_eq!(values["beta"], 0);
    assert_eq!(values["gamma"], 1);
    assert_eq!(values["alpha"], 2);

    let mut session = Runtime::new()
        .open_write_session(Project::open(Some(&config)).expect("reopen for mutation"))
        .expect("open write session");
    let old = RecordCoordinate::try_new("Item", "beta").expect("old coordinate");
    let new = RecordCoordinate::try_new("Item", "delta").expect("new coordinate");
    let report = coflow_runtime::commands::apply_project_mutation(
        &mut session,
        MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::RenameRecord {
                record: old,
                file: None,
                new_id: new.id.to_string(),
            }],
        },
    )
    .expect("rename id key");
    assert!(report.write_ok && report.check_ok);
    assert!(report
        .affected_files
        .contains(&"data/items.cfd".to_string()));
    assert!(report.written_files.contains(&"data/items.cfd".to_string()));
    assert!(report
        .written_files
        .contains(&"coflow.enum.lock.json".to_string()));
    let values = active_enum_values(&project_dir);
    assert_eq!(values["delta"], 0);
    assert!(!values.contains_key("beta"));

    let flag_dir = write_id_as_enum_project(
        true,
        "first: Item { name: \"First\" }\nsecond: Item { name: \"Second\" }\nthird: Item { name: \"Third\" }\n",
    );
    let flag_project =
        Project::open(Some(&flag_dir.path().join("coflow.yaml"))).expect("open flag enum project");
    coflow_runtime::commands::generate_project_code(&flag_project).expect("generate flag enum");
    let values = active_enum_values(&flag_dir);
    assert_eq!(values["first"], 1);
    assert_eq!(values["second"], 2);
    assert_eq!(values["third"], 4);
    let generated = fs::read_to_string(flag_dir.path().join("generated/csharp/ItemId.cs"))
        .expect("generated flag enum");
    assert!(generated.contains("[global::System.Flags]"));
    assert!(generated.contains("first = 1L"));
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
    assert!(session.queries().source_files().all(|path| {
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cfd"))
    }));
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
                "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: {output}\n"
            ),
        )
        .expect("config");
        let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open");
        let error = coflow_runtime::commands::generate_project_code(&opened)
            .expect_err("overlapping output must be rejected");
        assert!(
            error.to_string().contains("overlap") || error.to_string().contains("project root")
        );
        assert_eq!(fs::read_to_string(&marker).expect("marker remains"), "keep");
    }
}

#[test]
fn codegen_publishes_to_an_external_relative_directory() {
    let parent = tempfile::tempdir().expect("parent");
    let root = parent.path().join("project");
    fs::create_dir(&root).expect("root");
    fs::write(root.join("schema.cft"), "table Item { name: string; }\n").expect("schema");
    fs::write(root.join("data.cfd"), "Item { sword { name: \"Sword\" } }\n").expect("data");
    fs::write(root.join("coflow.yaml"), "schema: schema.cft\ndata: data.cfd\ncodegen: [{language: csharp, dir: ../generated}]\n").expect("config");
    let opened = Project::open(Some(&root.join("coflow.yaml"))).expect("open");
    coflow_runtime::commands::generate_project_code(&opened).expect("external codegen");
    assert!(parent.path().join("generated/Item.cs").is_file());
    coflow_runtime::commands::generate_project_code(&opened).expect("repeat external codegen");
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
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: data-alias/generated\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open");
    let error = coflow_runtime::commands::generate_project_code(&opened)
        .expect_err("symlinked overlapping output must be rejected");
    assert!(error.to_string().contains("overlaps data source"));
    assert!(!project.path().join("data/generated").exists());
}

#[test]
fn cli_codegen_dispatches_through_the_language_registry() {
    let project = write_project();
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    let outcome = coflow_runtime::commands::generate_project_code(&opened).expect("generate code");
    let coflow_runtime::commands::CommandOutcome::Success(report) = outcome else {
        panic!("C# generator should be registered for the smoke project");
    };
    assert_eq!(report.targets.len(), 1);
    assert!(project
        .path()
        .join("generated/csharp/CoflowData.cs")
        .is_file());
}

#[test]
fn csharp_codegen_rejects_removed_numeric_width_options() {
    let project = write_project();
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    int_32: true\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");

    let error = coflow_runtime::commands::generate_project_code(&opened)
        .expect_err("removed C# options must not be accepted");

    assert!(error.to_string().contains("unknown field `int_32`"));
    assert!(!project.path().join("generated/csharp").exists());
}

#[test]
fn csharp_codegen_applies_namespace_option() {
    let project = write_project();
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");

    coflow_runtime::commands::generate_project_code(&opened).expect("generate namespaced C#");
    let metadata = fs::read_to_string(project.path().join("generated/csharp/CoflowData.cs"))
        .expect("metadata");
    assert!(metadata
        .replace("\r\n", "\n")
        .contains("namespace Game.Config\n{"));
    assert!(metadata.contains("global::Coflow.Loader.CfdLoader.Load"));
}

#[test]
fn codegen_failure_does_not_publish_an_earlier_target() {
    let project = write_project();
    fs::write(
        project.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/first\n  - language: not-installed\n    dir: generated/second\n",
    )
    .expect("config");
    let opened = Project::open(Some(&project.path().join("coflow.yaml"))).expect("open data");
    assert!(coflow_runtime::commands::generate_project_code(&opened).is_err());
    assert!(!project.path().join("generated/first").exists());
    assert!(!project.path().join("generated/second").exists());
}

#[test]
fn build_status_is_read_only_and_tracks_generated_contents() {
    let project_dir = write_project();
    let project =
        Project::open(Some(&project_dir.path().join("coflow.yaml"))).expect("open project");

    let status =
        coflow_runtime::commands::build_project_status(&project).expect("inspect build status");
    assert!(matches!(
        status,
        coflow_runtime::commands::CommandOutcome::Success(true)
    ));
    assert!(!project_dir.path().join("generated/csharp").exists());
    assert!(!project_dir.path().join(".coflow/artifacts").exists());

    coflow_runtime::commands::generate_project_code(&project).expect("generate code");
    let status =
        coflow_runtime::commands::build_project_status(&project).expect("inspect clean status");
    assert!(matches!(
        status,
        coflow_runtime::commands::CommandOutcome::Success(false)
    ));

    fs::write(
        project_dir.path().join("generated/csharp/CoflowData.cs"),
        "changed",
    )
    .expect("change generated output");
    let status =
        coflow_runtime::commands::build_project_status(&project).expect("inspect changed status");
    assert!(matches!(
        status,
        coflow_runtime::commands::CommandOutcome::Success(true)
    ));
}

#[test]
fn generated_status_ignores_line_endings_and_preserves_equivalent_files() {
    let dir = write_project();
    let project = Project::open(Some(dir.path())).expect("project");
    coflow_runtime::commands::generate_project_code(&project).expect("generate");
    let output = dir.path().join("generated/csharp/CoflowData.cs");
    let original = fs::read_to_string(&output).expect("generated text");
    assert!(!original.contains('\r'));
    assert!(original.ends_with('\n') && !original.ends_with("\n\n"));
    for text in [
        original.replace('\n', "\r\n"),
        original.trim_end_matches('\n').to_string(),
        format!("{original}\n\n"),
    ] {
        fs::write(&output, &text).expect("rewrite endings");
        assert!(matches!(
            coflow_runtime::commands::build_project_status(&project).expect("status"),
            coflow_runtime::commands::CommandOutcome::Success(false)
        ));
        coflow_runtime::commands::generate_project_code(&project).expect("generate again");
        assert_eq!(fs::read_to_string(&output).expect("preserved text"), text);
        let other = dir.path().join("generated/csharp/Item.cs");
        fs::write(&other, "changed").expect("change other file");
        coflow_runtime::commands::generate_project_code(&project).expect("repair other file");
        assert_eq!(
            fs::read_to_string(&output).expect("preserved equivalent text"),
            text
        );
    }
    fs::write(&output, original.replacen('\n', " \n", 1)).expect("trailing space change");
    assert!(matches!(
        coflow_runtime::commands::build_project_status(&project).expect("status"),
        coflow_runtime::commands::CommandOutcome::Success(true)
    ));
}

#[test]
fn codegen_preserves_unity_meta_without_generation_history() {
    let project_dir = write_project();
    let config = project_dir.path().join("coflow.yaml");
    let project = Project::open(Some(&config)).expect("open project");
    coflow_runtime::commands::generate_project_code(&project).expect("generate baseline");

    fs::write(
        project_dir.path().join("generated/csharp/Item.cs.meta"),
        "unity-guid",
    )
    .expect("write Unity metadata");

    coflow_runtime::commands::generate_project_code(&project).expect("reuse unchanged output");

    fs::write(
        project_dir.path().join("schema.cft"),
        "table Item { name: string; level: int = 1; }\n",
    )
    .expect("change schema");
    let changed = Project::open(Some(&config)).expect("reopen changed project");
    coflow_runtime::commands::generate_project_code(&changed).expect("generate changed output");
    assert_eq!(
        fs::read_to_string(project_dir.path().join("generated/csharp/Item.cs.meta"))
            .expect("read preserved Unity metadata"),
        "unity-guid"
    );
}

fn dimension_project(singleton: bool, overlay: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("dimension project");
    let modifier = if singleton { "singleton" } else { "table" };
    fs::write(
        dir.path().join("schema.cft"),
        format!("{modifier} UiText {{ @localized welcome: string; @localized farewell: string; }}"),
    )
    .expect("schema");
    fs::create_dir_all(dir.path().join("data")).expect("data dir");
    let key = if singleton { "UiText" } else { "main" };
    fs::write(
        dir.path().join("data/base.cfd"),
        format!("{key}: UiText {{ welcome: \"Hello\", farewell: \"Bye\" }}"),
    )
    .expect("base CFD");
    fs::write(dir.path().join("data/ja.cfd"), overlay).expect("overlay CFD");
    fs::write(dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n"
    ).expect("config");
    dir
}

#[test]
fn csharp_codegen_emits_shared_loader_descriptors_for_dimensions() {
    let dir = dimension_project(
        false,
        "language[ja] { main: UiText { welcome: \"こんにちは\" } }",
    );
    let project = Project::open(Some(dir.path())).expect("project");
    coflow_runtime::commands::generate_project_code(&project).expect("generate");
    let root = fs::read_to_string(dir.path().join("generated/csharp/CoflowData.cs")).expect("root");
    let ty = fs::read_to_string(dir.path().join("generated/csharp/UiText.cs")).expect("type");
    assert!(root.contains("CfdValues.Dimension(\"language\""));
    assert!(ty.contains("public string welcome"));
    assert!(!root.contains("data/ja.cfd"));
    assert!(!root.contains("__coflow_"));
}

#[test]
fn csharp_codegen_uses_one_singleton_business_type_for_dimensions() {
    let dir = dimension_project(
        true,
        "language[ja] { UiText: UiText { welcome: \"こんにちは\", farewell: \"さようなら\" } }",
    );
    let project = Project::open(Some(dir.path())).expect("project");
    coflow_runtime::commands::generate_project_code(&project).expect("generate");
    let root = fs::read_to_string(dir.path().join("generated/csharp/CoflowData.cs")).expect("root");
    assert!(root.contains("CfdTypeKind.Singleton"));
    assert_eq!(root.matches("CfdValues.Dimension(\"language\"").count(), 2);
    assert!(!root.contains("__coflow_"));
}

#[test]
fn project_reports_missing_overlay_targets_at_the_original_field() {
    let dir = dimension_project(
        false,
        "language[ja] { missing: UiText { welcome: \"こんにちは\" } }",
    );
    let project = Project::open(Some(dir.path())).expect("project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("session");
    let diagnostics = session.into_diagnostics();
    let diagnostic = diagnostics
        .diagnostics
        .iter()
        .find(|d| d.code == "REF-001")
        .expect("missing target");
    assert!(
        matches!(diagnostic.primary.as_ref().map(|label| &label.location),
        Some(coflow_runtime::SourceLocation::FileSpan { path, start_line: 0, .. })
        if path.ends_with("data/ja.cfd"))
    );
}

#[test]
fn project_discovers_arbitrary_variants_from_selected_files() {
    let dir = dimension_project(
        true,
        "language[ja] { UiText: UiText { welcome: \"こんにちは\" } } language[future] {}",
    );
    let project = Project::open(Some(dir.path())).expect("project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("session");
    assert_eq!(
        session
            .model()
            .dimension_variants("language")
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["future", "ja"]
    );
    assert!(session.into_diagnostics().is_empty());
}

#[test]
fn editor_rules_do_not_execute_in_data_sessions_or_block_cli_generation() {
    let dir = write_project();
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; check Valid { false : \"editor business issue\"; } }",
    )
    .expect("schema with check");
    let project = Project::open(Some(dir.path())).expect("project");
    let data = Runtime::new()
        .open_data_session(project.clone())
        .expect("data session");
    assert!(!data.queries().has_diagnostics());
    assert_eq!(data.queries().execution_stats().check_roots_executed, 0);
    let editor = Runtime::new()
        .open_read_only_session(project.clone())
        .expect("editor session");
    assert!(editor.queries().has_diagnostics());
    assert!(editor
        .into_diagnostics()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "editor business issue"));
    assert!(matches!(
        coflow_runtime::commands::check_project(&project).expect("CLI data check"),
        coflow_runtime::commands::CommandOutcome::Success(_)
    ));
    assert!(matches!(
        coflow_runtime::commands::generate_project_code(&project).expect("generate"),
        coflow_runtime::commands::CommandOutcome::Success(_)
    ));
    let root = fs::read_to_string(dir.path().join("generated/csharp/CoflowData.cs"))
        .expect("generated loader");
    assert!(!root.contains("editor business issue"));
    assert!(!root.contains("CftSchema"));
}
