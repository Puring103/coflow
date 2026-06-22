#![allow(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use coflow_api::{
    DataLoader, DiagnosticSet, LoadContext, LoadedRecords, LoaderDescriptor, ProbeResult,
    ProjectSourceRef, ResolvedSource, SourceLocationSpec, SourceResolveContext,
};
use common::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn check_project_passes_for_rpg_example() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_returns_schema_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-schema-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("bad.cft"),
        "type Broken { value: Missing; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("coflow.yaml"), "schema: schema/\nsources: []\n")
        .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Missing")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn check_project_returns_diagnostic_set_not_json_dto() {
    let root = temp_project_dir("coflow-pipeline-diagnostic-set");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\nsources:\n  - path: ''\n",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics");
    };
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "PROJECT-001" && diagnostic.message == "sources[0].path is empty"
    }));
}

#[test]
fn check_project_validates_excel_sources_before_schema_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-check-source-before-schema");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("bad.cft"),
        "type Broken { value: Missing; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/missing.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("missing source diagnostics");

    assert_diagnostic_message_contains(
        outcome,
        "sources[0].path `data/missing.xlsx` does not exist",
    );
}

#[test]
fn check_project_excel_open_diagnostic_contains_file_path() {
    let root = temp_project_dir("coflow-pipeline-excel-open-path");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("bad.xlsx"), "not an xlsx").expect("write bad xlsx");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/bad.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXCEL-OPEN"
                && matches!(
                    diagnostic.primary.as_ref().map(|label| &label.location),
                    Some(
                        SourceLocation::FileSpan { path, .. }
                            | SourceLocation::TableCell { path, .. }
                    )
                        if path.ends_with("bad.xlsx")
                )),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn check_project_returns_check_diagnostics_after_successful_load() {
    let root = temp_project_dir("coflow-pipeline-check-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    write_invalid_check_project(&root).expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected check diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "CFD-CHECK-001"
                && diagnostic.stage == "CHECK"
                && matches!(
                    diagnostic.primary.as_ref().map(|label| &label.location),
                    Some(SourceLocation::TableCell {
                        path,
                        sheet: Some(sheet),
                        row: 2,
                        column: 2,
                    }) if path.ends_with("configs.xlsx") && sheet == "Item"
                )),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn check_project_collects_load_diagnostics_from_multiple_sources() {
    let root = temp_project_dir("coflow-pipeline-multiple-load-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("bad_a.cfd"),
        "item_a Item { value: 1 }\n",
    )
    .expect("write bad cfd a");
    std::fs::write(
        root.join("data").join("bad_b.cfd"),
        "item_b Item { value: 2 }\n",
    )
    .expect("write bad cfd b");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/bad_a.cfd
  - path: data/bad_b.cfd
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected load diagnostics");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.primary.as_ref().map(|label| &label.location),
            Some(SourceLocation::FileSpan { path, .. }) if path.ends_with("bad_a.cfd")
        )),
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.primary.as_ref().map(|label| &label.location),
            Some(SourceLocation::FileSpan { path, .. }) if path.ends_with("bad_b.cfd")
        )),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn check_project_defaults_excel_sheets_when_source_omits_sheet_config() {
    let root = temp_project_dir("coflow-pipeline-default-excel-sheets");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
                check { name != ""; }
            }
        "#,
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    write_item_workbook(&workbook_path, None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/configs.xlsx
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_loads_directory_sources_and_resolves_excel_cfd_refs_both_ways() {
    let root = temp_project_dir("coflow-pipeline-mixed-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
                linked_stage: Stage? = null;

                check {
                    name != "";
                    when linked_stage != null {
                        linked_stage.reward_item.id == id;
                    }
                }
            }

            type Stage {
                name: string;
                reward_item: Item;

                check {
                    name != "";
                    reward_item.id != "";
                }
            }
        "#,
    )
    .expect("write schema");
    write_item_workbook(
        &root.join("data").join("configs.xlsx"),
        Some("@Stage.forest"),
    )
    .expect("write workbook");
    std::fs::write(
        root.join("data").join("stages.cfd"),
        r#"
            forest: Stage {
                name: "Forest",
                reward_item: @Item.potion,
            }
        "#,
    )
    .expect("write cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_treats_source_extensions_case_sensitively() {
    let root = temp_project_dir("coflow-pipeline-source-extension-case");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data").join("dir")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
            }
        "#,
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("dir").join("CONFIGS.XLSX"), None)
        .expect("write uppercase workbook");
    std::fs::write(
        root.join("data").join("dir").join("IGNORED.CFD"),
        r#"
            ignored: Item {
                name: "Ignored",
            }
        "#,
    )
    .expect("write uppercase cfd source");
    std::fs::write(
        root.join("data").join("IGNORED.CFD"),
        r#"
            ignored_explicit: Item {
                name: "Ignored explicit",
            }
        "#,
    )
    .expect("write explicit uppercase cfd source");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            item_1: Item {
                name: "Item",
            }
        "#,
    )
    .expect("write lowercase cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/dir
  - path: data/items.cfd
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));

    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/IGNORED.CFD
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config with explicit uppercase source");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let outcome = check_project(&project).expect("check project");

    assert_diagnostic_message_contains(outcome, "has no matching loader");
}

#[test]
fn check_project_accepts_path_field_that_points_to_a_directory_source() {
    let root = temp_project_dir("coflow-pipeline-file-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_ignores_unsupported_files_inside_directory_sources() {
    let root = temp_project_dir("coflow-pipeline-directory-ignores-unsupported");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(root.join("data").join("notes.txt"), "ignored\n")
        .expect("write unsupported file");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_reports_source_with_both_path_and_url() {
    let root = temp_project_dir("coflow-pipeline-source-both-path-and-url");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/configs.xlsx
    url: https://example.test/configs
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let err = Project::open_schema_only(Some(root.as_path()))
        .expect_err("path and url should fail to parse");
    assert!(
        err.contains("source must set exactly one of `path` or `url`"),
        "error: {err}"
    );
}

#[test]
fn check_project_reports_explicit_file_with_unsupported_extension() {
    let root = temp_project_dir("coflow-pipeline-unsupported-file-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(root.join("data").join("notes.txt"), "not a data file\n")
        .expect("write unsupported source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/notes.txt
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert_diagnostic_message_contains(outcome, "has no matching loader");
}

#[test]
fn check_project_allows_mixed_directory_source_with_excel_sheets_config() {
    let root = temp_project_dir("coflow-pipeline-cfd-sheets-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("data").join("extra.cfd"),
        r#"
            extra: Item {
                name: "Extra",
            }
        "#,
    )
    .expect("write cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
    sheets:
      - sheet: Item
        columns:
          id: id
          name: name
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_lets_loader_resolve_directory_sources_without_extension_registration() {
    let root = temp_project_dir("coflow-pipeline-loader-resolve-directory");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(root.join("data").join("item.custom"), "ignored").expect("write custom data");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - type: custom-dir
    path: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let mut registry = test_registry();
    let loads = Arc::new(AtomicUsize::new(0));
    registry
        .register_loader(CustomDirLoader {
            loads: Arc::clone(&loads),
            fail_preflight: false,
        })
        .expect("register custom loader");

    let outcome = coflow_pipeline::check_project(&project, &registry).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}

#[test]
fn check_project_runs_loader_preflight_before_load_and_stops_on_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-loader-preflight-before-load");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(root.join("data").join("item.custom"), "ignored").expect("write custom data");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - type: custom-dir
    path: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let mut registry = test_registry();
    let loads = Arc::new(AtomicUsize::new(0));
    registry
        .register_loader(CustomDirLoader {
            loads: Arc::clone(&loads),
            fail_preflight: true,
        })
        .expect("register custom loader");

    let outcome = coflow_pipeline::check_project(&project, &registry).expect("check project");

    assert_diagnostic_message_contains(outcome, "custom preflight failed");
    assert_eq!(loads.load(Ordering::SeqCst), 0);
}

#[derive(Debug, Clone)]
struct CustomDirLoader {
    loads: Arc<AtomicUsize>,
    fail_preflight: bool,
}

const CUSTOM_DIR_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "custom-dir",
    display_name: "Custom directory loader",
    extensions: &[],
    uri_schemes: &[],
    option_keys: &[],
};

impl DataLoader for CustomDirLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CUSTOM_DIR_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(CUSTOM_DIR_DESCRIPTOR.id) {
            ProbeResult::certain()
        } else {
            ProbeResult::none()
        }
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            return Ok(Vec::new());
        };
        Ok(vec![ResolvedSource {
            provider_id: CUSTOM_DIR_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Path(path.join("item.custom")),
            options: source.options.clone(),
            display_name: "item.custom".to_string(),
        }])
    }

    fn load(
        &self,
        _ctx: LoadContext<'_>,
        _source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(LoadedRecords {
            records: Vec::new(),
        })
    }

    fn preflight(&self, _ctx: LoadContext<'_>, _source: &ResolvedSource) -> DiagnosticSet {
        if self.fail_preflight {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "CUSTOM-PREFLIGHT",
                "CUSTOM",
                "custom preflight failed",
            ))
        } else {
            DiagnosticSet::empty()
        }
    }
}
