#![allow(clippy::expect_used, clippy::panic)]

use std::process::Command;

#[test]
fn cell_value_is_part_of_table_loader_core_not_a_standalone_crate() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read workspace manifest");

    assert!(
        !manifest.contains("crates/coflow-cell-value"),
        "cell value parsing belongs under coflow-loader-table-core, not a standalone workspace crate"
    );
    assert!(
        !std::path::Path::new("crates/coflow-cell-value/Cargo.toml").exists(),
        "coflow-cell-value crate should not exist"
    );
    assert!(
        manifest.contains("crates/coflow-loader-table-core"),
        "workspace should include coflow-loader-table-core"
    );
    assert!(
        std::path::Path::new("crates/coflow-loader-table-core/src/cell_value/mod.rs").exists(),
        "cell value parsing should live in coflow-loader-table-core"
    );
}

#[test]
fn final_architecture_has_no_pipeline_or_editor_core_crates() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read workspace manifest");

    assert!(
        manifest.contains("crates/coflow-builtins"),
        "default provider registration should live in coflow-builtins"
    );
    assert!(
        manifest.contains("crates/coflow-engine"),
        "shared project runtime should live in coflow-engine"
    );
    assert!(
        !manifest.contains("crates/coflow-pipeline"),
        "coflow-pipeline should be removed after runtime moves to engine and CLI artifacts move to the root crate"
    );
    assert!(
        !manifest.contains("crates/coflow-editor-core"),
        "editor backend core should live inside editors/cfd-editor/src-tauri, not as a standalone crate"
    );
    assert!(
        !std::path::Path::new("crates/coflow-pipeline/Cargo.toml").exists(),
        "coflow-pipeline crate should not exist"
    );
    assert!(
        !std::path::Path::new("crates/coflow-editor-core/Cargo.toml").exists(),
        "coflow-editor-core crate should not exist"
    );
}

#[test]
fn provider_shared_algorithms_do_not_live_in_coflow_api() {
    let api =
        std::fs::read_to_string("crates/coflow-api/src/lib.rs").expect("read coflow-api source");

    for forbidden in [
        "pub mod table",
        "pub mod cell_value",
        "pub mod export",
        "export_model_with_encoder",
        "ExportEncoder",
    ] {
        assert!(
            !api.contains(forbidden),
            "coflow-api should not expose provider implementation algorithm `{forbidden}`"
        );
    }
}

#[test]
fn cli_diagnostic_json_does_not_live_in_coflow_project() {
    let project = std::fs::read_to_string("crates/coflow-project/src/lib.rs")
        .expect("read coflow-project source");
    let cli_diagnostics =
        std::fs::read_to_string("src/diagnostics.rs").expect("read CLI diagnostics source");

    for forbidden in [
        "DiagnosticJson",
        "RelatedJson",
        "diagnostic_json_from_set",
        "pub use coflow_api::SourceLocationSpec",
        "schema_diagnostics(",
        "data_diagnostics(",
        "codegen_diagnostics(",
    ] {
        assert!(
            !project.contains(forbidden),
            "CLI diagnostic output DTO `{forbidden}` should not live in coflow-project"
        );
    }
    assert!(
        cli_diagnostics.contains("pub struct DiagnosticJson"),
        "CLI diagnostic JSON DTO should live in the root coflow crate"
    );
}

#[test]
fn table_provider_algorithms_are_not_reexported_by_excel_loader() {
    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/lib.rs")
        .expect("read excel loader");

    for forbidden in [
        "pub struct ExcelLoadOutput",
        "pub fn load_excel_model",
        "pub fn load_excel(",
        "pub struct TableSource",
        "pub fn collect_table_input_records",
        "pub use coflow_loader_table_core::TableSheet",
        "shared_table_source_from_excel_table_source",
    ] {
        assert!(
            !excel.contains(forbidden),
            "Excel loader should not expose table-core facade `{forbidden}`"
        );
    }
}

#[test]
fn editor_backend_does_not_depend_on_checker_runtime_directly() {
    let manifest = std::fs::read_to_string("editors/cfd-editor/src-tauri/Cargo.toml")
        .expect("read editor backend manifest");

    assert!(
        !manifest.contains("coflow-checker"),
        "editor backend should consume checker results through coflow-engine, not depend on coflow-checker directly"
    );
}

#[test]
fn engine_public_api_does_not_expose_checker_dependency_graph() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");

    assert!(
        !engine.contains("use coflow_checker::{run_checks_with_deps, DependencyGraph}"),
        "coflow-engine should wrap checker dependency graph instead of re-exporting the checker type through ProjectSession"
    );
    assert!(
        !engine.contains("pub dependencies: coflow_checker::DependencyGraph")
            && !engine.contains("pub dependencies: DependencyGraph,"),
        "ProjectSession should not expose the checker crate dependency graph type directly"
    );
    assert!(
        !engine.contains("impl From<coflow_checker::DependencyGraph> for DependencyIndex"),
        "checker dependency graph conversion should stay as an engine-internal helper, not a public From impl"
    );
}

#[test]
fn loaders_do_not_depend_on_checker_runtime_directly() {
    let excel_manifest = std::fs::read_to_string("crates/coflow-loader-excel/Cargo.toml")
        .expect("read excel loader manifest");

    assert!(
        !excel_manifest.contains("coflow-checker"),
        "loaders should only produce input records; model checks belong in coflow-engine"
    );
}

#[test]
fn lsp_is_documented_as_schema_only_not_engine_runtime_host() {
    let plan = std::fs::read_to_string("docs/spec/15-final-architecture-refactor-plan.md")
        .expect("read final architecture plan");

    assert!(
        !plan.contains("LSP --> Engine"),
        "final dependency graph should not show coflow-lsp depending on coflow-engine while the LSP remains schema-only"
    );
    assert!(
        !plan.contains("LSP --> Builtins"),
        "final dependency graph should not show coflow-lsp depending on builtins/provider registry while the LSP remains schema-only"
    );
    assert!(
        plan.contains("schema-only"),
        "final architecture plan should explicitly document why coflow-lsp does not depend on engine/builtins"
    );
}

#[test]
fn table_writers_use_shared_cell_renderer() {
    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")
        .expect("read excel writer");
    let lark =
        std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs").expect("read lark loader");

    assert!(
        excel.contains("coflow_loader_table_core::cell_value::{render_cell_value"),
        "Excel writer should use the shared table-core cell renderer"
    );
    assert!(
        lark.contains("coflow_loader_table_core::cell_value::{render_cell_value"),
        "Lark writer should use the shared table-core cell renderer"
    );
    for forbidden in ["fn render_cell_value(value:", "fn render_lark_cell_value"] {
        assert!(
            !excel.contains(forbidden),
            "Excel writer should not duplicate shared renderer `{forbidden}`"
        );
        assert!(
            !lark.contains(forbidden),
            "Lark writer should not duplicate shared renderer `{forbidden}`"
        );
    }
}

#[test]
fn tracked_files_do_not_include_generated_outputs() {
    let output = Command::new("git")
        .args(["ls-files"])
        .output()
        .expect("run git ls-files");
    if !output.status.success() {
        return;
    }

    let stdout = String::from_utf8(output.stdout).expect("git output is utf8");
    let offenders = stdout
        .lines()
        .filter(|path| path.contains("/generated/") && std::path::Path::new(path).exists())
        .collect::<Vec<_>>();

    assert!(
        offenders.is_empty(),
        "generated outputs should not be tracked: {offenders:#?}"
    );
}
