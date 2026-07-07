#![allow(clippy::expect_used, clippy::panic)]

use std::process::Command;
use toml::{Table, Value};

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
fn engine_runtime_indexes_do_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let indexes = std::fs::read_to_string("crates/coflow-engine/src/indexes.rs")
        .expect("read engine indexes source");

    for expected in [
        "pub struct DiagnosticsStore",
        "pub struct SourceIndex",
        "pub struct RecordIndex",
        "pub struct FileIndex",
        "pub struct DependencyIndex",
    ] {
        assert!(
            indexes.contains(expected),
            "engine runtime index type `{expected}` should live in indexes.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine runtime index type `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn engine_session_api_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let session = std::fs::read_to_string("crates/coflow-engine/src/session.rs")
        .expect("read engine session source");

    for expected in [
        "pub struct RecordCoordinate",
        "pub struct ProjectSession",
        "impl ProjectSession",
        "pub struct ProjectSchemaSession",
    ] {
        assert!(
            session.contains(expected),
            "engine session API `{expected}` should live in session.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine session API `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn engine_schema_build_pipeline_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let schema_build = std::fs::read_to_string("crates/coflow-engine/src/schema_build.rs")
        .expect("read engine schema build source");

    for expected in [
        "pub fn build_project_schema_session",
        "fn validate_dimension_schema_config",
        "fn compile_project_schema",
        "fn diagnostics_from_schema_build",
    ] {
        assert!(
            schema_build.contains(expected),
            "engine schema build helper `{expected}` should live in schema_build.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine schema build helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn engine_runtime_does_not_depend_on_excel_implementation_crates() {
    let manifest =
        std::fs::read_to_string("crates/coflow-engine/Cargo.toml").expect("read engine manifest");
    let data_files = std::fs::read_to_string("crates/coflow-engine/src/data_files.rs")
        .expect("read engine data file commands");

    for forbidden in ["calamine", "umya-spreadsheet", "umya_spreadsheet"] {
        assert!(
            !manifest.contains(forbidden),
            "coflow-engine should not depend on Excel implementation crate `{forbidden}`"
        );
        assert!(
            !data_files.contains(forbidden),
            "data file commands should use provider table operations instead of `{forbidden}`"
        );
    }
}

#[test]
fn engine_data_file_commands_do_not_depend_on_cfd_provider_writer() {
    let manifest =
        std::fs::read_to_string("crates/coflow-engine/Cargo.toml").expect("read engine manifest");
    let production_manifest = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("manifest has production dependency section");
    let data_files = std::fs::read_to_string("crates/coflow-engine/src/data_files.rs")
        .expect("read engine data file commands");
    let dimension_regenerate =
        std::fs::read_to_string("crates/coflow-engine/src/dimensions/regenerate.rs")
            .expect("read engine dimension regeneration");

    for forbidden in [
        "coflow-loader-cfd",
        "coflow-loader-csv",
        "coflow-loader-table-core",
    ] {
        assert!(
            !production_manifest.contains(forbidden),
            "coflow-engine should use provider operations instead of depending on {forbidden}"
        );
    }
    for forbidden in [
        "coflow_loader_cfd",
        "coflow_loader_csv",
        "coflow_loader_table_core",
        "parse_cfd",
        "CfdBlockEntry",
        "render_cell_value",
    ] {
        assert!(
            !data_files.contains(forbidden),
            "data file commands should not contain CFD provider implementation detail `{forbidden}`"
        );
        assert!(
            !dimension_regenerate.contains(forbidden),
            "dimension regeneration should use provider operations instead of `{forbidden}`"
        );
    }
    for forbidden in ["DataFileProvider", "fs::write", "create_dir_all"] {
        assert!(
            !data_files.contains(forbidden),
            "data file commands should use provider table operations instead of `{forbidden}`"
        );
    }
}

#[test]
fn data_model_schema_projection_uses_cft_schema_view() {
    let schema_view = std::fs::read_to_string("crates/coflow-data-model/src/schema_view.rs")
        .expect("read data-model schema view");

    assert!(
        schema_view.contains("CftSchemaView::new(schema)"),
        "data-model schema projection should be built from coflow-cft CftSchemaView"
    );
    for forbidden in [
        "schema.all_types()",
        "schema.all_enums()",
        "CftSchemaType,",
        "CftSchemaEnum,",
        "CftSchemaField,",
    ] {
        assert!(
            !schema_view.contains(forbidden),
            "data-model schema view should not rebuild schema projection from `{forbidden}`"
        );
    }
}

#[test]
fn data_model_value_semantics_uses_cft_schema_view() {
    let value_semantics = std::fs::read_to_string("crates/coflow-data-model/src/value_semantics.rs")
        .expect("read data-model value semantics");

    assert!(
        value_semantics.contains("CftSchemaView::new(schema)"),
        "data-model value semantics should use coflow-cft CftSchemaView as its schema query"
    );
    for forbidden in [
        ".resolve_type(",
        ".has_enum(",
        ".has_type(",
        ".all_fields",
    ] {
        assert!(
            !value_semantics.contains(forbidden),
            "data-model value semantics should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn csharp_codegen_schema_projection_uses_cft_schema_view() {
    let schema_view = std::fs::read_to_string("crates/coflow-codegen-csharp/src/schema_view.rs")
        .expect("read C# codegen schema view");
    let ir =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/ir.rs").expect("read C# IR");
    let emit =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");
    let codegen = format!("{schema_view}\n{ir}\n{emit}");

    assert!(
        schema_view.contains("CftSchemaView::new(schema)"),
        "C# codegen schema projection should be built from coflow-cft CftSchemaView"
    );
    for forbidden in [
        "schema.all_types()",
        "schema.all_enums()",
        "pub fn all_types",
        "pub fn all_enums",
        "pub enums:",
        "view.enums",
        "CftSchemaType,",
        "CftSchemaEnum,",
        "CftSchemaField,",
    ] {
        assert!(
            !codegen.contains(forbidden),
            "C# codegen should not rebuild schema projection from `{forbidden}`"
        );
    }
}

#[test]
fn exporter_core_schema_projection_uses_cft_schema_view() {
    let exporter =
        std::fs::read_to_string("crates/coflow-exporter-core/src/lib.rs").expect("read exporter");

    assert!(
        exporter.contains("CftSchemaView::new(schema)"),
        "exporter core schema traversal should be built from coflow-cft CftSchemaView"
    );
    for forbidden in [
        "schema.all_types()",
        "schema.all_enums()",
        "schema.resolve_type(",
        "CftSchemaField",
        "struct FieldMeta",
    ] {
        assert!(
            !exporter.contains(forbidden),
            "exporter core should not rebuild schema traversal from `{forbidden}`"
        );
    }
}

#[test]
fn engine_dimension_synthesis_uses_cft_schema_view() {
    let synthesize = std::fs::read_to_string("crates/coflow-engine/src/dimensions/synthesize.rs")
        .expect("read dimension synthesis");

    assert!(
        synthesize.contains("CftSchemaView::new(schema)"),
        "dimension synthesis should derive schema metadata from coflow-cft CftSchemaView"
    );
    for forbidden in ["schema.all_types()", "schema.resolve_type("] {
        assert!(
            !synthesize.contains(forbidden),
            "dimension synthesis should not rebuild schema traversal from `{forbidden}`"
        );
    }
}

#[test]
fn engine_schema_inspect_uses_cft_schema_view_for_schema_traversal() {
    let schema_inspect = std::fs::read_to_string("crates/coflow-engine/src/schema_inspect.rs")
        .expect("read schema inspect");

    assert!(
        schema_inspect.contains("CftSchemaView::new(&session.schema)"),
        "schema inspect should use coflow-cft CftSchemaView for schema traversal"
    );
    for forbidden in [
        ".schema\n        .all_types()",
        ".schema\n        .all_enums()",
        ".schema.resolve_type(",
    ] {
        assert!(
            !schema_inspect.contains(forbidden),
            "schema inspect should not traverse raw schema via `{forbidden}`"
        );
    }
}

#[test]
fn engine_write_rules_use_cft_schema_view_for_path_types() {
    let write_rules = std::fs::read_to_string("crates/coflow-engine/src/write_rules.rs")
        .expect("read engine write rules");

    assert!(
        write_rules.contains("CftSchemaView::new(schema)"),
        "engine write rules should use coflow-cft CftSchemaView for schema path lookup"
    );
    for forbidden in [".resolve_type(", ".all_fields", ".has_enum(", ".has_type("] {
        assert!(
            !write_rules.contains(forbidden),
            "engine write rules should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_data_file_headers_use_cft_schema_view() {
    let data_files = std::fs::read_to_string("crates/coflow-engine/src/data_files.rs")
        .expect("read engine data file commands");

    assert!(
        data_files.contains("CftSchemaView::new(&session.schema)"),
        "data file header planning should use CftSchemaView for schema metadata"
    );
    for forbidden in [".resolve_type(", "session.schema.resolve_type"] {
        assert!(
            !data_files.contains(forbidden),
            "data file commands should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_writes_use_cft_schema_view_for_insert_schema_checks() {
    let writes = std::fs::read_to_string("crates/coflow-engine/src/writes.rs")
        .expect("read engine writes");

    assert!(
        writes.contains("CftSchemaView::new(&session.schema)"),
        "engine writes should use CftSchemaView for insert schema checks"
    );
    for forbidden in ["session.schema.resolve_type", ".all_fields"] {
        assert!(
            !writes.contains(forbidden),
            "engine writes should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_mutation_defaults_use_cft_schema_view() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation.rs")
        .expect("read mutation")
        .replace("\r\n", "\n");

    let default_section = mutation
        .split("fn default_record_for_type(")
        .nth(1)
        .and_then(|rest| rest.split("fn coerce_mutation_value(").next())
        .expect("mutation default materialization section");

    assert!(
        default_section.contains("CftSchemaView::new(schema)"),
        "mutation default materialization should use coflow-cft CftSchemaView"
    );
    for forbidden in [
        ".resolve_type(",
        ".resolve_enum(",
        ".has_type(",
        ".has_enum(",
        "CftSchemaField",
    ] {
        assert!(
            !default_section.contains(forbidden),
            "mutation default materialization should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_mutation_field_coercion_uses_cft_schema_view() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation.rs")
        .expect("read mutation")
        .replace("\r\n", "\n");

    let field_coercion_section = mutation
        .split("fn prepare_provided_insert_fields(")
        .nth(1)
        .and_then(|rest| rest.split("#[derive(Debug, Clone)]\nstruct ExpectedValue").next())
        .expect("mutation field coercion section");

    assert!(
        field_coercion_section.contains("CftSchemaView::new(&session.schema)"),
        "mutation field coercion should use coflow-cft CftSchemaView for field lookup"
    );
    for forbidden in [
        "CftSchemaField",
        "session.schema.resolve_type(",
        ".resolve_type(actual_type)",
        ".all_fields\n            .iter()",
    ] {
        assert!(
            !field_coercion_section.contains(forbidden),
            "mutation field coercion should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_mutation_uses_cft_schema_view_for_schema_queries() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation.rs")
        .expect("read mutation");

    assert!(
        mutation.contains("CftSchemaView::new(&session.schema)")
            || mutation.contains("CftSchemaView::new(schema)"),
        "mutation schema queries should go through coflow-cft CftSchemaView"
    );
    for forbidden in [
        ".resolve_type(",
        ".resolve_enum(",
        ".has_type(",
        ".has_enum(",
        "CftSchemaField",
    ] {
        assert!(
            !mutation.contains(forbidden),
            "mutation should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_public_mutation_api_does_not_expose_prepared_ops() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation.rs")
        .expect("read mutation")
        .replace("\r\n", "\n");

    assert!(
        !engine.contains("PreparedMutationOp"),
        "prepared mutation ops are an engine-internal execution detail and must not be re-exported"
    );
    assert!(
        !mutation.contains("pub enum PreparedMutationOp"),
        "external callers must not be able to construct prepared ops that bypass mutation validation"
    );
    let prepared_struct = mutation
        .split("pub struct PreparedMutation {")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("PreparedMutation struct");
    assert!(
        !prepared_struct.contains("pub "),
        "PreparedMutation should be opaque; found public field in `{prepared_struct}`"
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
fn workspace_members_inherit_workspace_lints() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read workspace manifest");
    let manifest = manifest.parse::<Table>().expect("parse workspace manifest");
    let members = manifest["workspace"]["members"]
        .as_array()
        .expect("workspace members");

    let missing = members
        .iter()
        .filter_map(Value::as_str)
        .filter(|member| {
            let manifest_path = std::path::Path::new(member).join("Cargo.toml");
            let member_manifest = std::fs::read_to_string(&manifest_path)
                .unwrap_or_else(|err| panic!("read {}: {err}", manifest_path.display()));
            let member_manifest = member_manifest
                .parse::<Table>()
                .unwrap_or_else(|err| panic!("parse {}: {err}", manifest_path.display()));
            !member_manifest
                .get("lints")
                .and_then(Value::as_table)
                .and_then(|lints| lints.get("workspace"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "workspace members must inherit workspace lints with `[lints] workspace = true`: {missing:#?}"
    );
}

#[test]
fn lsp_is_documented_as_schema_only_not_engine_runtime_host() {
    let architecture = std::fs::read_to_string("website/docs/docs/reference/12-architecture.md")
        .expect("read architecture reference");

    assert!(
        !architecture.contains("LSP --> Engine"),
        "architecture reference should not show coflow-lsp depending on coflow-engine while the LSP remains schema-only"
    );
    assert!(
        !architecture.contains("LSP --> Builtins"),
        "architecture reference should not show coflow-lsp depending on builtins/provider registry while the LSP remains schema-only"
    );
    assert!(
        architecture.contains("schema-only"),
        "architecture reference should explicitly document why coflow-lsp does not depend on engine/builtins"
    );
}

#[test]
fn table_writers_use_shared_cell_renderer() {
    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")
        .expect("read excel writer");
    let table_writer = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer.rs")
        .expect("read table writer");
    let lark =
        std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs").expect("read lark loader");

    assert!(
        excel.contains("coflow_loader_table_core::writer::{")
            && table_writer.contains("crate::cell_value::{render_cell_value"),
        "Excel writer should use the shared table-core writer and renderer"
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
fn editor_backend_entrypoints_do_not_use_crash_error_handling() {
    for path in [
        "editors/cfd-editor/src-tauri/src/lib.rs",
        "editors/cfd-editor/src-tauri/src/main.rs",
    ] {
        let source = std::fs::read_to_string(path).expect("read editor backend entrypoint");
        for forbidden in [
            ".expect(",
            ".unwrap(",
            "panic!",
            "todo!",
            "unimplemented!",
            "dbg!",
        ] {
            assert!(
                !source.contains(forbidden),
                "editor backend entrypoint `{path}` should return or report errors instead of using `{forbidden}`"
            );
        }
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
