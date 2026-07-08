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
        "runtime implementation should live in coflow-engine"
    );
    assert!(
        manifest.contains("crates/coflow-runtime"),
        "host-facing runtime boundary should live in coflow-runtime"
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
fn api_registry_is_split_by_responsibility() {
    let registry =
        std::fs::read_to_string("crates/coflow-api/src/registry.rs").expect("read API registry");
    let selection = std::fs::read_to_string("crates/coflow-api/src/registry/selection.rs")
        .expect("read API registry selection");
    let errors = std::fs::read_to_string("crates/coflow-api/src/registry/errors.rs")
        .expect("read API registry errors");

    for expected in [
        "pub fn select_source_provider",
        "SourceProviderSelectionError::UnknownSourceProvider",
        "SourceProviderSelectionError::AmbiguousSourceProviders",
    ] {
        assert!(
            selection.contains(expected),
            "API registry selection item `{expected}` should live in registry/selection.rs"
        );
        assert!(
            !registry.contains(expected),
            "API registry selection item `{expected}` should not live in registry.rs"
        );
    }
    for expected in [
        "pub enum SourceProviderSelectionError",
        "pub struct ProviderRegistrationError",
        "impl std::error::Error for ProviderRegistrationError",
    ] {
        assert!(
            errors.contains(expected),
            "API registry error item `{expected}` should live in registry/errors.rs"
        );
        assert!(
            !registry.contains(expected),
            "API registry error item `{expected}` should not live in registry.rs"
        );
    }
    assert!(
        registry.lines().count() < 260,
        "coflow-api registry.rs should stay focused on provider storage and lookup"
    );
    for expected in [
        "source_providers:",
        "source_writers:",
        "pub fn register_source_provider",
        "pub fn register_source_writer",
        "pub fn source_provider(",
        "pub fn source_writer(",
        "pub fn source_provider_descriptors",
        "pub fn source_writer_descriptors",
    ] {
        assert!(
            registry.contains(expected),
            "API registry should expose source provider/writer item `{expected}`"
        );
    }
    for forbidden in [
        "DataLoader",
        "DataWriter",
        "register_loader",
        "register_writer",
        "pub fn loader(",
        "pub fn writer(",
        "pub fn loader_descriptors",
        "pub fn writer_descriptors",
        "    loaders: BTreeMap",
        "    writers: BTreeMap",
    ] {
        assert!(
            !registry.contains(forbidden),
            "API registry should use source provider/writer naming instead of `{forbidden}`"
        );
    }
}

#[test]
fn api_writer_contract_is_split_by_responsibility() {
    let writer =
        std::fs::read_to_string("crates/coflow-api/src/writer.rs").expect("read API writer");
    let capabilities = std::fs::read_to_string("crates/coflow-api/src/writer/capabilities.rs")
        .expect("read API writer capabilities");
    let requests = std::fs::read_to_string("crates/coflow-api/src/writer/requests.rs")
        .expect("read API writer requests");

    for expected in [
        "pub struct WriterDescriptor",
        "pub struct WriterCapabilities",
        "impl WriterCapabilities",
    ] {
        assert!(
            capabilities.contains(expected),
            "API writer capability item `{expected}` should live in writer/capabilities.rs"
        );
        assert!(
            !writer.contains(expected),
            "API writer capability item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub enum WriteFieldPathSegment",
        "pub struct WriteCellRequest",
        "pub struct InsertRecordRequest",
        "pub struct DeleteRecordRequest",
        "pub struct RenameRecordRequest",
        "pub struct RewriteRecordReferencesRequest",
        "pub struct SpreadRewriteTarget",
        "pub struct WriteOutcome",
        "pub struct WriteContext",
    ] {
        assert!(
            requests.contains(expected),
            "API writer request item `{expected}` should live in writer/requests.rs"
        );
        assert!(
            !writer.contains(expected),
            "API writer request item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.contains("pub trait SourceWriter"),
        "API writer trait should remain in writer.rs"
    );
    assert!(
        !writer.contains("pub trait DataWriter"),
        "API writer contract should be named SourceWriter"
    );
    for forbidden in ["CreateTableRequest", "fn create_table"] {
        assert!(
            !writer.contains(forbidden),
            "table creation should live on TableManager, not SourceWriter: `{forbidden}`"
        );
    }
    assert!(
        writer.lines().count() < 170,
        "coflow-api writer.rs should stay focused on the SourceWriter trait"
    );
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
fn root_cli_arguments_do_not_live_in_main_rs() {
    let main = std::fs::read_to_string("src/main.rs").expect("read root CLI main");
    let cli = std::fs::read_to_string("src/cli.rs").expect("read root CLI argument definitions");
    let output =
        std::fs::read_to_string("src/cli_output.rs").expect("read root CLI output helpers");
    let data_get_target = std::fs::read_to_string("src/data_get_target.rs")
        .expect("read root CLI data get target helpers");

    for expected in [
        "pub(crate) struct Cli",
        "pub(crate) enum Command",
        "pub(crate) struct BuildArgs",
        "pub(crate) enum DataCommand",
        "pub(crate) struct DataWriteFileArgs",
    ] {
        assert!(
            cli.contains(expected),
            "root CLI argument item `{expected}` should live in cli.rs"
        );
        assert!(
            !main.contains(expected),
            "root CLI argument item `{expected}` should not live in main.rs"
        );
    }
    assert!(
        main.lines().count() < 800,
        "root CLI main.rs should stay below the 800-line large-module threshold"
    );
    assert!(
        cli.lines().count() < 800,
        "root CLI cli.rs should stay below the 800-line large-module threshold"
    );
    for expected in [
        "pub(crate) fn write_project_diagnostics",
        "pub(crate) fn write_cli_error",
        "fn write_diagnostic_block",
        "pub(crate) fn relativize_message_paths",
        "fn slash_path",
    ] {
        assert!(
            output.contains(expected),
            "root CLI output helper `{expected}` should live in cli_output.rs"
        );
        assert!(
            !main.contains(expected),
            "root CLI output helper `{expected}` should not live in main.rs"
        );
    }
    for expected in [
        "pub(crate) struct DataGetTarget",
        "pub(crate) fn parse_data_get_target",
        "fn looks_like_record_selector",
        "fn parse_record_selector",
    ] {
        assert!(
            data_get_target.contains(expected),
            "root CLI data get target helper `{expected}` should live in data_get_target.rs"
        );
        assert!(
            !main.contains(expected),
            "root CLI data get target helper `{expected}` should not live in main.rs"
        );
    }
}

#[test]
fn root_cli_artifact_safety_does_not_live_in_commands_rs() {
    let commands = std::fs::read_to_string("src/commands.rs").expect("read root CLI commands");
    let artifact_safety = std::fs::read_to_string("src/commands/artifact_safety.rs")
        .expect("read root CLI artifact safety helpers");

    for expected in [
        "pub(super) struct ArtifactOutputPlan",
        "pub(super) fn artifact_safety_diagnostics",
        "fn output_scope_diagnostics",
        "fn overlapping_output_diagnostics",
        "fn configured_source_paths",
    ] {
        assert!(
            artifact_safety.contains(expected),
            "root CLI artifact safety item `{expected}` should live in commands/artifact_safety.rs"
        );
        assert!(
            !commands.contains(expected),
            "root CLI artifact safety item `{expected}` should not live in commands.rs"
        );
    }
    assert!(
        commands.lines().count() < 800,
        "root CLI commands.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn root_cli_id_as_enum_lockfile_does_not_live_in_commands_rs() {
    let commands = std::fs::read_to_string("src/commands.rs").expect("read root CLI commands");
    let id_as_enum = std::fs::read_to_string("src/commands/id_as_enum.rs")
        .expect("read root CLI id-as-enum helpers");

    for expected in [
        "pub(super) fn id_as_enum_variants_for_schema_only",
        "pub(super) fn stage_id_as_enum_lockfile_for_build",
        "fn merge_id_as_enum_lockfile",
        "fn allocate_id_as_enum_value",
        "fn read_id_as_enum_lockfile",
        "fn lockfile_to_variants",
        "fn annotation_name_arg",
    ] {
        assert!(
            id_as_enum.contains(expected),
            "root CLI @idAsEnum item `{expected}` should live in commands/id_as_enum.rs"
        );
        assert!(
            !commands.contains(expected),
            "root CLI @idAsEnum item `{expected}` should not live in commands.rs"
        );
    }
    assert!(
        commands.lines().count() < 800,
        "root CLI commands.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn root_cli_artifact_staging_does_not_live_in_artifacts_rs() {
    let artifacts = std::fs::read_to_string("src/artifacts.rs").expect("read root CLI artifacts");
    let staging = std::fs::read_to_string("src/artifacts/staging.rs")
        .expect("read root CLI artifact staging");

    for expected in [
        "pub struct StagedArtifactDir",
        "pub struct StagedArtifactFile",
        "pub(super) fn stage_artifact_set",
        "pub fn commit_staged_dir_and_file",
        "pub fn commit_staged_dirs_and_file",
        "fn safe_artifact_path",
        "fn replace_file_with_staging",
        "fn replace_dir_with_staging",
        "fn rollback_committed_dirs",
        "fn unique_sidecar_path",
    ] {
        assert!(
            staging.contains(expected),
            "root CLI artifact staging item `{expected}` should live in artifacts/staging.rs"
        );
        assert!(
            !artifacts.contains(expected),
            "root CLI artifact staging item `{expected}` should not live in artifacts.rs"
        );
    }
    assert!(
        artifacts.lines().count() < 400,
        "root CLI artifacts.rs should stay focused on artifact orchestration"
    );
}

#[test]
fn data_command_helpers_do_not_live_in_data_commands_rs() {
    let commands =
        std::fs::read_to_string("src/data_commands.rs").expect("read root CLI data commands");
    let lark = std::fs::read_to_string("src/data_commands/lark.rs")
        .expect("read data command lark helpers");
    let files = std::fs::read_to_string("src/data_commands/files.rs")
        .expect("read data command file helpers");
    let output = std::fs::read_to_string("src/data_commands/output.rs")
        .expect("read data command output helpers");
    let write_file = std::fs::read_to_string("src/data_commands/write_file.rs")
        .expect("read data command write-file helpers");

    for expected in [
        "pub(super) fn infer_table_provider",
        "pub(super) fn create_lark_table",
        "struct CliTableLayout",
        "fn lark_table_layout",
        "fn matching_lark_sheet_config",
    ] {
        assert!(
            lark.contains(expected),
            "data command Lark helper `{expected}` should live in data_commands/lark.rs"
        );
        assert!(
            !commands.contains(expected),
            "data command Lark helper `{expected}` should not live in data_commands.rs"
        );
    }
    for expected in [
        "pub(super) fn create_file_report",
        "pub(super) fn create_table_report",
        "pub(super) fn sync_header_report",
    ] {
        assert!(
            files.contains(expected),
            "data command file helper `{expected}` should live in data_commands/files.rs"
        );
        assert!(
            !commands.contains(expected),
            "data command file helper `{expected}` should not live in data_commands.rs"
        );
    }
    for expected in [
        "pub(super) fn write_json",
        "pub(super) fn write_sources_human",
        "pub(super) fn write_list_human",
        "pub(super) fn write_get_human",
        "pub(super) fn write_patch_human",
        "pub(super) fn write_file_report_human",
        "pub(super) fn write_data_write_file_human",
        "fn write_flat_diagnostics",
        "pub(super) fn flat_diagnostics",
        "pub(super) fn file_error_report",
    ] {
        assert!(
            output.contains(expected),
            "data command output helper `{expected}` should live in data_commands/output.rs"
        );
        assert!(
            !commands.contains(expected),
            "data command output helper `{expected}` should not live in data_commands.rs"
        );
    }
    for expected in [
        "pub(super) fn run_write_file",
        "struct DataWriteTarget",
        "fn resolve_data_write_target",
        "fn is_within_configured_local_data_source",
        "fn read_stdin_source",
        "fn check_project_after_data_write",
    ] {
        assert!(
            write_file.contains(expected),
            "data command write-file helper `{expected}` should live in data_commands/write_file.rs"
        );
        assert!(
            !commands.contains(expected),
            "data command write-file helper `{expected}` should not live in data_commands.rs"
        );
    }
    assert!(
        commands.lines().count() < 800,
        "root CLI data_commands.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn project_config_deserialization_does_not_live_in_project_lib_rs() {
    let project =
        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");
    let config = std::fs::read_to_string("crates/coflow-project/src/config.rs")
        .expect("read project config");
    let validation = std::fs::read_to_string("crates/coflow-project/src/validation.rs")
        .expect("read project validation");

    for expected in [
        "pub struct ProjectConfig",
        "pub struct SourceConfig",
        "pub struct OutputConfig",
        "struct NoDuplicateValue",
        "impl<'de> Deserialize<'de> for ProjectConfig",
    ] {
        assert!(
            config.contains(expected),
            "project config item `{expected}` should live in config.rs"
        );
        assert!(
            !project.contains(expected),
            "project config item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub(super) struct ProjectDiagnostic",
        "pub(super) fn validate_project_config_schema_only_collecting",
        "pub(super) fn validate_sources_collecting",
        "pub(super) fn validate_for_codegen_collecting",
    ] {
        assert!(
            validation.contains(expected),
            "project validation item `{expected}` should live in validation.rs"
        );
        assert!(
            !project.contains(expected),
            "project validation item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        project.lines().count() < 800,
        "coflow-project lib.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn project_schema_discovery_does_not_live_in_project_lib_rs() {
    let project =
        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");
    let schema_sources = std::fs::read_to_string("crates/coflow-project/src/schema_sources.rs")
        .expect("read project schema sources");

    for expected in [
        "pub struct SchemaFile",
        "pub(super) fn schema_files",
        "fn push_schema_path",
        "fn collect_cft_files",
        "fn is_cft_path",
    ] {
        assert!(
            schema_sources.contains(expected),
            "project schema discovery item `{expected}` should live in schema_sources.rs"
        );
        assert!(
            !project.contains(expected),
            "project schema discovery item `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn project_path_helpers_do_not_live_in_project_lib_rs() {
    let project =
        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");
    let paths =
        std::fs::read_to_string("crates/coflow-project/src/paths.rs").expect("read project paths");

    for expected in [
        "pub fn resolve_config_path",
        "pub(super) fn resolve_project_relative",
        "fn find_default_config",
        "fn is_yaml_path",
        "pub fn path_to_slash",
        "pub fn normalize_path",
    ] {
        assert!(
            paths.contains(expected),
            "project path helper `{expected}` should live in paths.rs"
        );
        assert!(
            !project.contains(expected),
            "project path helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn project_diagnostic_conversion_does_not_live_in_project_lib_rs() {
    let project =
        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");
    let diagnostics = std::fs::read_to_string("crates/coflow-project/src/diagnostics.rs")
        .expect("read project diagnostics");

    for expected in [
        "pub fn dedupe_cft_diagnostics",
        "fn cft_diagnostic_key",
        "pub fn diagnostic_set_from_cft",
        "fn diagnostic_from_cft",
        "fn cft_label_range",
        "fn byte_position",
        "pub(super) fn project_diagnostics_to_set",
        "pub(super) fn join_diagnostic_messages",
    ] {
        assert!(
            diagnostics.contains(expected),
            "project diagnostic helper `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !project.contains(expected),
            "project diagnostic helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn table_provider_algorithms_are_not_reexported_by_excel_source_provider() {
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
fn excel_loader_options_do_not_live_in_lib_rs() {
    let lib =
        std::fs::read_to_string("crates/coflow-loader-excel/src/lib.rs").expect("read excel lib");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-excel/src/diagnostics.rs")
        .expect("read excel diagnostics");
    let options = std::fs::read_to_string("crates/coflow-loader-excel/src/options.rs")
        .expect("read excel options parser");
    let source = std::fs::read_to_string("crates/coflow-loader-excel/src/source.rs")
        .expect("read excel source");

    for expected in [
        "pub(super) fn excel_sheets_from_options",
        "fn excel_sheet_from_value",
        "fn optional_string_field",
    ] {
        assert!(
            options.contains(expected),
            "Excel option parser item `{expected}` should live in options.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel option parser item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct ExcelDiagnostics",
        "pub struct ExcelDiagnostic",
        "pub struct ExcelLabel",
        "pub struct ExcelLocation",
        "pub fn map_label_with_record_offset",
        "pub(crate) fn excel_diagnostics_to_api",
        "fn table_code_to_excel",
        "fn excel_label_to_api",
    ] {
        assert!(
            diagnostics.contains(expected),
            "Excel diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct ExcelSource",
        "pub struct ExcelSheet",
        "pub struct ExcelInputRecords",
        "pub fn collect_input_records",
        "fn table_sources_from_excel",
        "fn table_source_from_excel",
        "fn cell_text",
        "fn unsupported_cell_diagnostic",
    ] {
        assert!(
            source.contains(expected),
            "Excel source item `{expected}` should live in source.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel source item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        lib.lines().count() < 360,
        "coflow-loader-excel lib.rs should stay below the 360-line focused-module threshold"
    );
}

#[test]
fn csv_dimension_source_sync_does_not_live_in_writer_rs() {
    let writer =
        std::fs::read_to_string("crates/coflow-loader-csv/src/writer.rs").expect("read csv writer");
    let dimensions = std::fs::read_to_string("crates/coflow-loader-csv/src/writer/dimensions.rs")
        .expect("read csv writer dimension source sync");
    let plan = std::fs::read_to_string("crates/coflow-loader-csv/src/writer/plan.rs")
        .expect("read csv writer plan helpers");
    let table_manager =
        std::fs::read_to_string("crates/coflow-loader-csv/src/writer/table_manager.rs")
            .expect("read csv writer table manager");

    for expected in [
        "impl DimensionSourceManager for CsvWriter",
        "fn sync_dimension_source",
        "struct DimensionCsvRow",
        "fn render_dimension_csv_value",
    ] {
        assert!(
            dimensions.contains(expected),
            "CSV dimension source item `{expected}` should live in writer/dimensions.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV dimension source item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) fn apply_plan",
        "fn mutate_csv",
        "fn set_csv_cell",
        "fn ensure_expected_key",
    ] {
        assert!(
            plan.contains(expected),
            "CSV writer plan item `{expected}` should live in writer/plan.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV writer plan item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "impl TableManager for CsvWriter",
        "pub static CSV_TABLE_MANAGER_DESCRIPTOR",
        "fn create_table",
        "fn sync_header",
        "fn added_columns",
        "fn removed_columns",
        "fn sync_rows_to_header",
    ] {
        assert!(
            table_manager.contains(expected),
            "CSV table manager item `{expected}` should live in writer/table_manager.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV table manager item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.lines().count() < 360,
        "coflow-loader-csv writer.rs should stay below the 360-line focused-module threshold"
    );
}

#[test]
fn excel_table_manager_does_not_live_in_writer_rs() {
    let writer = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")
        .expect("read excel writer");
    let table_manager =
        std::fs::read_to_string("crates/coflow-loader-excel/src/writer/table_manager.rs")
            .expect("read excel table manager");

    for expected in [
        "impl TableManager for ExcelWriter",
        "pub static EXCEL_TABLE_MANAGER_DESCRIPTOR",
        "fn create_table",
        "fn sync_header",
        "fn create_excel_file",
        "fn append_excel_sheet",
        "fn sync_excel_header",
    ] {
        assert!(
            table_manager.contains(expected),
            "Excel table manager item `{expected}` should live in writer/table_manager.rs"
        );
        assert!(
            !writer.contains(expected),
            "Excel table manager item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.lines().count() < 650,
        "coflow-loader-excel writer.rs should stay below the 650-line focused-module threshold"
    );
}

#[test]
fn csv_loader_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-csv/src/lib.rs").expect("read csv lib");
    let format =
        std::fs::read_to_string("crates/coflow-loader-csv/src/format.rs").expect("read csv format");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-csv/src/diagnostics.rs")
        .expect("read csv diagnostics");
    let options = std::fs::read_to_string("crates/coflow-loader-csv/src/options.rs")
        .expect("read csv options");
    let source =
        std::fs::read_to_string("crates/coflow-loader-csv/src/source.rs").expect("read csv source");

    for expected in ["pub fn parse", "pub fn write", "fn write_cell"] {
        assert!(
            format.contains(expected),
            "CSV format item `{expected}` should live in format.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV format item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub(super) fn csv_sheets_from_options",
        "fn csv_sheet_from_value",
        "fn optional_string_field",
    ] {
        assert!(
            options.contains(expected),
            "CSV option parser item `{expected}` should live in options.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV option parser item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct CsvDiagnostics",
        "pub struct CsvDiagnostic",
        "pub struct CsvLocation",
        "pub fn csv_diagnostics_to_api",
        "fn csv_label_to_api",
        "fn table_code_to_csv",
    ] {
        assert!(
            diagnostics.contains(expected),
            "CSV diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct CsvSource",
        "pub struct CsvSheet",
        "pub struct CsvInputRecords",
        "pub fn collect_input_records",
        "fn table_sources_from_csv",
        "fn table_source_from_csv",
        "fn default_sheet_name",
    ] {
        assert!(
            source.contains(expected),
            "CSV source item `{expected}` should live in source.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV source item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        lib.lines().count() < 800,
        "coflow-loader-csv lib.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_writer_is_split_by_responsibility() {
    let writer =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/writer.rs").expect("read cfd writer");
    let dimensions = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/dimensions.rs")
        .expect("read cfd writer dimension source sync");
    let patch = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/patch.rs")
        .expect("read cfd writer patch helpers");
    let render = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/render.rs")
        .expect("read cfd writer render helpers");
    let schema_nav = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/schema_nav.rs")
        .expect("read cfd writer schema navigation helpers");
    let target = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/target.rs")
        .expect("read cfd writer target locator helpers");

    for expected in [
        "impl DimensionSourceManager for CfdWriter",
        "fn sync_dimension_source",
        "struct DimensionCfdRow",
        "fn read_existing_dimension_cfd",
    ] {
        assert!(
            dimensions.contains(expected),
            "CFD dimension source item `{expected}` should live in writer/dimensions.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD dimension source item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) fn apply_patch",
        "pub(super) fn find_record",
        "pub(super) fn replace_spans",
    ] {
        assert!(
            patch.contains(expected),
            "CFD patch item `{expected}` should live in writer/patch.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD patch item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) enum WriteTarget",
        "pub(super) fn locate_target",
        "pub(super) fn spread_entries_at_path",
        "fn block_entries_at_path",
    ] {
        assert!(
            target.contains(expected),
            "CFD target locator item `{expected}` should live in writer/target.rs"
        );
        assert!(
            !patch.contains(expected) && !writer.contains(expected),
            "CFD target locator item `{expected}` should not live in writer.rs or writer/patch.rs"
        );
    }
    for expected in [
        "pub(super) fn cfd_top_level_fields",
        "pub(super) fn rewrite_cfd_records",
        "pub(super) fn serialize_value",
        "pub(super) fn serialize_value_for_type",
    ] {
        assert!(
            render.contains(expected),
            "CFD render item `{expected}` should live in writer/render.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD render item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        schema_nav.contains("pub(super) fn type_after_field_segment")
            && schema_nav.contains("pub(super) fn dict_key_path_matches"),
        "CFD writer schema navigation helpers should live in writer/schema_nav.rs"
    );
    assert!(
        writer.lines().count() < 800,
        "coflow-loader-cfd writer.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_loader_parser_and_diagnostics_do_not_live_in_lib_rs() {
    let lib =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/lib.rs").expect("read cfd loader");
    let parser =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/parser.rs").expect("read cfd parser");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-cfd/src/diagnostics.rs")
        .expect("read cfd diagnostics");

    for expected in [
        "pub(super) struct Parser",
        "pub(super) struct ParsedCfdInputRecord",
        "fn parse_records_with_spans",
    ] {
        assert!(
            parser.contains(expected),
            "CFD parser item `{expected}` should live in parser.rs"
        );
        assert!(
            !lib.contains(expected),
            "CFD parser item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub enum CfdTextLoadError",
        "pub struct CfdTextDiagnostics",
        "pub struct CfdTextDiagnostic",
        "pub enum CfdTextErrorCode",
        "pub(super) fn cfd_error_to_diagnostics",
    ] {
        assert!(
            diagnostics.contains(expected),
            "CFD diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "CFD diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        lib.lines().count() < 800,
        "coflow-loader-cfd lib.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_parser_schema_helpers_do_not_live_in_parser_rs() {
    let parser =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/parser.rs").expect("read cfd parser");
    let schema = std::fs::read_to_string("crates/coflow-loader-cfd/src/parser/schema.rs")
        .expect("read cfd parser schema helpers");

    for expected in [
        "pub(super) struct FieldMeta",
        "pub(super) struct ParsedObjectFields",
        "pub(super) fn validate_record_key",
        "pub(super) fn validate_actual_type",
        "pub(super) fn full_fields",
        "fn field_meta",
    ] {
        assert!(
            schema.contains(expected),
            "CFD parser schema helper `{expected}` should live in parser/schema.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFD parser schema helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 800,
        "coflow-loader-cfd parser.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_parser_lexer_helpers_do_not_live_in_parser_rs() {
    let parser =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/parser.rs").expect("read cfd parser");
    let lexer = std::fs::read_to_string("crates/coflow-loader-cfd/src/parser/lexer.rs")
        .expect("read cfd parser lexer helpers");

    for expected in [
        "pub(super) enum NameTokenKind",
        "pub(super) struct ScalarToken",
        "pub(super) fn parse_scalar_token",
        "pub(super) fn parse_quoted_string",
        "pub(super) fn skip_ws_and_comments",
        "pub(super) fn eat_keyword",
        "pub(super) fn is_value_boundary",
    ] {
        assert!(
            lexer.contains(expected),
            "CFD parser lexer helper `{expected}` should live in parser/lexer.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFD parser lexer helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 800,
        "coflow-loader-cfd parser.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_parser_value_helpers_do_not_live_in_parser_rs() {
    let parser =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/parser.rs").expect("read cfd parser");
    let value = std::fs::read_to_string("crates/coflow-loader-cfd/src/parser/value.rs")
        .expect("read cfd parser value helpers");

    for expected in [
        "pub(super) fn parse_value",
        "fn parse_int",
        "fn parse_dict_key",
        "pub(super) fn parse_object_value",
        "fn parse_ref_value",
        "fn parse_spread_value",
    ] {
        assert!(
            value.contains(expected),
            "CFD parser value helper `{expected}` should live in parser/value.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFD parser value helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 500,
        "coflow-loader-cfd parser.rs should stay focused on record-level parsing"
    );
}

#[test]
fn cfd_syntax_parser_token_helpers_are_split_out() {
    let parser =
        std::fs::read_to_string("crates/coflow-cfd/src/parser.rs").expect("read CFD syntax parser");
    let tokens = std::fs::read_to_string("crates/coflow-cfd/src/parser/tokens.rs")
        .expect("read CFD syntax parser token helpers");

    for expected in [
        "pub(super) struct Token",
        "pub(super) fn parse_key",
        "fn parse_name_token",
        "pub(super) fn parse_quoted_string",
        "pub(super) fn skip_ws_and_comments",
        "fn is_value_boundary",
    ] {
        assert!(
            tokens.contains(expected),
            "CFD syntax parser token helper `{expected}` should live in parser/tokens.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFD syntax parser token helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 400,
        "coflow-cfd parser.rs should stay focused on CFD AST structure parsing"
    );
}

#[test]
fn editor_backend_does_not_depend_on_checker_runtime_directly() {
    let manifest = std::fs::read_to_string("editors/cfd-editor/src-tauri/Cargo.toml")
        .expect("read editor backend manifest");

    assert!(
        !manifest.contains("coflow-checker"),
        "editor backend should consume checker results through coflow-runtime, not depend on coflow-checker directly"
    );
}

#[test]
fn hosts_depend_on_runtime_boundary_not_engine_implementation() {
    let root_manifest = std::fs::read_to_string("Cargo.toml").expect("read root manifest");
    let editor_manifest = std::fs::read_to_string("editors/cfd-editor/src-tauri/Cargo.toml")
        .expect("read editor backend manifest");
    let runtime_manifest =
        std::fs::read_to_string("crates/coflow-runtime/Cargo.toml").expect("read runtime manifest");
    let runtime_lib =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read runtime lib");

    assert!(
        root_manifest.contains("coflow-runtime ="),
        "root CLI should depend on coflow-runtime"
    );
    assert!(
        !root_manifest.contains("coflow-engine ="),
        "root CLI should not depend on coflow-engine directly"
    );
    assert!(
        editor_manifest.contains("coflow-runtime ="),
        "editor backend should depend on coflow-runtime"
    );
    assert!(
        !editor_manifest.contains("coflow-engine ="),
        "editor backend should not depend on coflow-engine directly"
    );
    assert!(
        runtime_manifest.contains("coflow-engine ="),
        "runtime facade should delegate to the engine implementation crate"
    );
    assert!(
        runtime_lib.contains("pub use coflow_engine::*;"),
        "runtime facade should expose the current engine runtime API"
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
fn engine_load_pipeline_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let load =
        std::fs::read_to_string("crates/coflow-engine/src/load.rs").expect("read engine load");
    let session_build = std::fs::read_to_string("crates/coflow-engine/src/session_build.rs")
        .expect("read engine session build pipeline");

    for expected in [
        "pub(crate) struct ProjectLoadOutput",
        "pub(crate) struct LoadDiagnostics",
        "pub(crate) struct LoadProjectDataOptions",
        "pub(crate) fn load_project_data",
        "fn load_resolved_sources",
        "fn resolve_implicit_source",
        "fn run_project_checks",
        "fn resolve_sources",
        "fn logical_locations_from_cfd",
        "pub fn format_cfd_path",
    ] {
        assert!(
            load.contains(expected),
            "engine load pipeline item `{expected}` should live in load.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine load pipeline item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        engine.lines().count() < 300,
        "coflow-engine lib.rs should stay below the 300-line orchestration threshold"
    );
    for expected in [
        "pub fn build_project_session",
        "pub fn build_project_session_read_only",
        "fn build_project_session_with_dimension_mode",
        "fn build_schema_session",
        "fn build_data_pipeline",
        "fn load_base_data",
        "fn commit_dimensions_if_needed",
        "fn reload_with_dimensions",
        "fn rollback_dimensions_after_failed_pipeline",
        "fn assemble_session",
        "fn loader_extensions",
    ] {
        assert!(
            session_build.contains(expected),
            "engine session build item `{expected}` should live in session_build.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine session build item `{expected}` should not live in lib.rs"
        );
    }
    for forbidden in ["load_project_data(", "regenerate_dimension_sources("] {
        assert!(
            !engine.contains(forbidden),
            "engine lib.rs should not directly orchestrate `{forbidden}`"
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
fn engine_data_file_commands_do_not_depend_on_cfd_provider_source_writer() {
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
    let session_build = std::fs::read_to_string("crates/coflow-engine/src/session_build.rs")
        .expect("read engine session build pipeline");

    for expected in [
        "pub(crate) fn plan_dimension_generation",
        "pub(crate) fn commit_dimension_generation",
        "pub struct DimensionGenerationResult",
        "pub struct DimensionGenerationTransaction",
        "fn dimension_source_path",
        "fn dimension_entries",
    ] {
        assert!(
            dimension_regenerate.contains(expected),
            "dimension generation item `{expected}` should live in dimensions/regenerate.rs"
        );
    }
    for expected in [
        "commit_dimensions_if_needed",
        "reload_with_dimensions",
        "rollback_dimensions_after_failed_pipeline",
    ] {
        assert!(
            session_build.contains(expected),
            "session build should explicitly orchestrate dimension pipeline step `{expected}`"
        );
    }

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
    assert!(
        schema_view.contains("CftTypeMeta"),
        "data-model schema view should reuse coflow-cft type metadata instead of local type metadata"
    );
    assert!(
        schema_view.contains("CftFieldMeta") && schema_view.contains("CftSchemaTypeRef"),
        "data-model schema view should reuse coflow-cft field metadata and type refs"
    );
    for forbidden in [
        "schema.all_types()",
        "schema.all_enums()",
        "pub(crate) struct TypeMeta",
        "pub(crate) struct FieldMeta",
        "pub(crate) enum CfdType",
        "pub(crate) struct EnumMeta",
        "pub(crate) types:",
        "pub(crate) enums:",
        "children:",
        ".types.get(",
        ".types.values()",
        ".types.keys()",
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
    let value_semantics =
        std::fs::read_to_string("crates/coflow-data-model/src/value_semantics.rs")
            .expect("read data-model value semantics");

    assert!(
        value_semantics.contains("CftSchemaView::new(schema)"),
        "data-model value semantics should use coflow-cft CftSchemaView as its schema query"
    );
    for forbidden in [
        ".resolve_type(",
        ".has_enum(",
        ".all_fields",
        ".types.get(",
        ".enums.contains_key(",
    ] {
        assert!(
            !value_semantics.contains(forbidden),
            "data-model value semantics should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn data_model_runtime_model_is_split_by_responsibility() {
    let model =
        std::fs::read_to_string("crates/coflow-data-model/src/model.rs").expect("read model");
    let ids =
        std::fs::read_to_string("crates/coflow-data-model/src/model/ids.rs").expect("read ids");
    let domain = std::fs::read_to_string("crates/coflow-data-model/src/model/domain.rs")
        .expect("read domain");
    let dimensions = std::fs::read_to_string("crates/coflow-data-model/src/model/dimensions.rs")
        .expect("read dimensions");
    let edges =
        std::fs::read_to_string("crates/coflow-data-model/src/model/edges.rs").expect("read edges");
    let tables = std::fs::read_to_string("crates/coflow-data-model/src/model/tables.rs")
        .expect("read tables");
    let value =
        std::fs::read_to_string("crates/coflow-data-model/src/model/value.rs").expect("read value");
    let input =
        std::fs::read_to_string("crates/coflow-data-model/src/model/input.rs").expect("read input");

    for expected in [
        "pub struct CfdTypeId",
        "pub struct CfdDomainId",
        "pub struct CfdRecordId",
    ] {
        assert!(
            ids.contains(expected),
            "data-model id type `{expected}` should live in model/ids.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model id type `{expected}` should not live in model.rs"
        );
    }
    for expected in ["pub struct CfdDomainIndex"] {
        assert!(
            domain.contains(expected),
            "data-model domain type `{expected}` should live in model/domain.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model domain type `{expected}` should not live in model.rs"
        );
    }
    for expected in [
        "pub enum DimensionFieldLookupError",
        "pub struct DimensionFieldValue",
        "pub fn dimension_field_value",
    ] {
        assert!(
            dimensions.contains(expected),
            "data-model dimension helper `{expected}` should live in model/dimensions.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model dimension helper `{expected}` should not live in model.rs"
        );
    }
    for expected in [
        "pub struct RefSite",
        "pub struct RefEdge",
        "pub struct SpreadSite",
        "pub struct SpreadEdge",
    ] {
        assert!(
            edges.contains(expected),
            "data-model edge type `{expected}` should live in model/edges.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model edge type `{expected}` should not live in model.rs"
        );
    }
    for expected in ["pub struct CfdTable", "pub struct CfdPolymorphicIndex"] {
        assert!(
            tables.contains(expected),
            "data-model table type `{expected}` should live in model/tables.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model table type `{expected}` should not live in model.rs"
        );
    }
    for expected in [
        "pub struct CfdRecord",
        "pub struct CfdObject",
        "pub enum CfdValue",
        "pub enum CfdDictKey",
        "pub struct CfdEnumValue",
    ] {
        assert!(
            value.contains(expected),
            "data-model value type `{expected}` should live in model/value.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model value type `{expected}` should not live in model.rs"
        );
    }
    for expected in [
        "pub struct CfdInputRecord",
        "pub enum CfdInputValue",
        "pub enum CfdInputDictKey",
    ] {
        assert!(
            input.contains(expected),
            "data-model input type `{expected}` should live in model/input.rs"
        );
        assert!(
            !model.contains(expected),
            "data-model input type `{expected}` should not live in model.rs"
        );
    }
    assert!(
        model.lines().count() < 800,
        "coflow-data-model model.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn data_model_compiler_indexes_are_split_out() {
    let compiler = std::fs::read_to_string("crates/coflow-data-model/src/compiler.rs")
        .expect("read data-model compiler");
    let indexes = std::fs::read_to_string("crates/coflow-data-model/src/compiler/indexes.rs")
        .expect("read data-model compiler indexes");
    let resolve = std::fs::read_to_string("crates/coflow-data-model/src/compiler/resolve.rs")
        .expect("read data-model compiler resolve");
    let defaults = std::fs::read_to_string("crates/coflow-data-model/src/compiler/defaults.rs")
        .expect("read data-model compiler defaults");
    let validate = std::fs::read_to_string("crates/coflow-data-model/src/compiler/validate.rs")
        .expect("read data-model compiler validation");
    let dicts = std::fs::read_to_string("crates/coflow-data-model/src/compiler/validate/dicts.rs")
        .expect("read data-model compiler dict validation");

    for expected in [
        "pub(super) struct ModelIndexes",
        "pub(super) fn build_indexes",
        "fn add_polymorphic_ids",
        "pub(super) fn validate_singletons",
    ] {
        assert!(
            indexes.contains(expected),
            "data-model compiler index helper `{expected}` should live in compiler/indexes.rs"
        );
        assert!(
            !compiler.contains(expected),
            "data-model compiler index helper `{expected}` should not live in compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn resolve_fields",
        "fn resolve_value",
        "fn resolve_ref_target",
        "fn resolve_dict_spread",
        "fn resolve_spread_field",
    ] {
        assert!(
            resolve.contains(expected),
            "data-model compiler resolve helper `{expected}` should live in compiler/resolve.rs"
        );
        assert!(
            !compiler.contains(expected),
            "data-model compiler resolve helper `{expected}` should not live in compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn default_field_value",
        "fn default_value",
        "fn default_object_value",
        "fn push_default_type_mismatch",
        "fn non_nullable_type",
        "CftSchemaDefaultValue",
    ] {
        assert!(
            defaults.contains(expected),
            "data-model compiler default helper `{expected}` should live in compiler/defaults.rs"
        );
        assert!(
            !compiler.contains(expected),
            "data-model compiler default helper `{expected}` should not live in compiler.rs"
        );
    }
    for expected in [
        "pub(super) struct Validator",
        "pub(super) fn validate_record",
        "pub(super) fn validate_value",
        "fn top_level_spread_source",
    ] {
        assert!(
            validate.contains(expected),
            "data-model compiler validation helper `{expected}` should live in compiler/validate.rs"
        );
        assert!(
            !compiler.contains(expected),
            "data-model compiler validation helper `{expected}` should not live in compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn validate_dict_entries",
        "fn validate_dict_key",
    ] {
        assert!(
            dicts.contains(expected),
            "data-model compiler dict validation helper `{expected}` should live in compiler/validate/dicts.rs"
        );
        assert!(
            !validate.contains(expected),
            "data-model compiler dict validation helper `{expected}` should not live in compiler/validate.rs"
        );
        assert!(
            !compiler.contains(expected),
            "data-model compiler dict validation helper `{expected}` should not live in compiler.rs"
        );
    }
    assert!(
        compiler.lines().count() < 800,
        "coflow-data-model compiler.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cft_schema_compiler_symbols_are_split_out() {
    let compiler = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler.rs")
        .expect("read CFT schema compiler");
    let symbols = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/symbols.rs")
        .expect("read CFT schema compiler symbols");
    let annotations =
        std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/annotations.rs")
            .expect("read CFT schema compiler annotations");
    let build = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/build.rs")
        .expect("read CFT schema compiler build projection");
    let defaults = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/defaults.rs")
        .expect("read CFT schema compiler defaults");
    let types = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/types.rs")
        .expect("read CFT schema compiler type validation");

    for expected in [
        "pub(super) fn report_dangling_annotations",
        "pub(super) fn collect_symbols",
        "pub(super) fn validate_identifier",
        "fn insert_symbol",
        "pub(super) fn validate_enums",
    ] {
        assert!(
            symbols.contains(expected),
            "CFT schema compiler symbol helper `{expected}` should live in schema/compiler/symbols.rs"
        );
        assert!(
            !compiler.contains(expected),
            "CFT schema compiler symbol helper `{expected}` should not live in schema/compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn validate_annotations",
        "fn register_id_as_enum_name",
        "fn validate_annotation_list",
        "fn validate_field_annotations",
        "fn validate_id_as_enum_name",
    ] {
        assert!(
            annotations.contains(expected),
            "CFT schema compiler annotation helper `{expected}` should live in schema/compiler/annotations.rs"
        );
        assert!(
            !compiler.contains(expected),
            "CFT schema compiler annotation helper `{expected}` should not live in schema/compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn build_schema",
        "fn build_schema_field",
        "fn collect_all_schema_fields",
        "fn schema_default_value",
        "fn enum_variant_value",
        "fn localized_bucket",
    ] {
        assert!(
            build.contains(expected),
            "CFT schema compiler build helper `{expected}` should live in schema/compiler/build.rs"
        );
        assert!(
            !compiler.contains(expected),
            "CFT schema compiler build helper `{expected}` should not live in schema/compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn validate_defaults",
        "fn default_expr_type",
        "fn default_enum_variant_type",
    ] {
        assert!(
            defaults.contains(expected),
            "CFT schema compiler default helper `{expected}` should live in schema/compiler/defaults.rs"
        );
        assert!(
            !compiler.contains(expected),
            "CFT schema compiler default helper `{expected}` should not live in schema/compiler.rs"
        );
    }
    for expected in [
        "pub(super) fn validate_type_headers",
        "pub(super) fn validate_field_shapes",
        "pub(super) fn validate_inheritance",
        "fn detect_cycle",
        "pub(super) fn build_full_fields",
        "pub(super) fn ancestry_chain",
        "pub(super) fn resolve_field_type",
        "fn validate_field_type",
        "pub(super) fn collect_ancestor_fields",
    ] {
        assert!(
            types.contains(expected),
            "CFT schema compiler type helper `{expected}` should live in schema/compiler/types.rs"
        );
        assert!(
            !compiler.contains(expected),
            "CFT schema compiler type helper `{expected}` should not live in schema/compiler.rs"
        );
    }
    assert!(
        compiler.lines().count() < 260,
        "coflow-cft schema/compiler.rs should stay focused on phase orchestration and shared utilities"
    );
}

#[test]
fn cft_type_checker_operator_rules_are_split_out() {
    let type_checker = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker.rs")
        .expect("read CFT type checker");
    let ops = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker/ops.rs")
        .expect("read CFT type checker operators");

    for expected in [
        "pub(super) fn check_unary",
        "pub(super) fn check_binop",
        "pub(super) fn check_comparison",
        "fn operator_mismatch",
        "fn is_flag_enum",
    ] {
        assert!(
            ops.contains(expected),
            "CFT type checker operator helper `{expected}` should live in schema/type_checker/ops.rs"
        );
        assert!(
            !type_checker.contains(expected),
            "CFT type checker operator helper `{expected}` should not live in schema/type_checker.rs"
        );
    }
    assert!(
        type_checker.lines().count() < 800,
        "coflow-cft schema/type_checker.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cft_type_checker_function_rules_are_split_out() {
    let type_checker = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker.rs")
        .expect("read CFT type checker");
    let functions =
        std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker/functions.rs")
            .expect("read CFT type checker function rules");

    for expected in [
        "pub(super) fn check_call",
        "pub(super) fn check_method_call",
        "fn check_contains_method",
        "fn check_matches_method",
        "fn expect_arity",
    ] {
        assert!(
            functions.contains(expected),
            "CFT type checker function helper `{expected}` should live in schema/type_checker/functions.rs"
        );
        assert!(
            !type_checker.contains(expected),
            "CFT type checker function helper `{expected}` should not live in schema/type_checker.rs"
        );
    }
    assert!(
        type_checker.lines().count() < 500,
        "coflow-cft schema/type_checker.rs should stay focused on check expression dispatch"
    );
}

#[test]
fn cft_schema_view_dimension_check_analysis_is_split_out() {
    let schema_view =
        std::fs::read_to_string("crates/coflow-cft/src/schema_view.rs").expect("read schema view");
    let dimension_checks =
        std::fs::read_to_string("crates/coflow-cft/src/schema_view/dimension_checks.rs")
            .expect("read schema view dimension check analysis");
    let queries = std::fs::read_to_string("crates/coflow-cft/src/schema_view/queries.rs")
        .expect("read schema view query helpers");

    for expected in [
        "pub(super) fn dimension_checks_for_type",
        "struct DimensionCheckAnalyzer",
        "fn stmt_dimensions",
        "fn expr_usage",
        "fn type_ref_to_check_ty",
    ] {
        assert!(
            dimension_checks.contains(expected),
            "CFT schema view dimension check helper `{expected}` should live in schema_view/dimension_checks.rs"
        );
        assert!(
            !schema_view.contains(expected),
            "CFT schema view dimension check helper `{expected}` should not live in schema_view.rs"
        );
    }
    assert!(
        schema_view.lines().count() < 500,
        "coflow-cft schema_view.rs should stay focused on schema view metadata/query"
    );
    for expected in [
        "pub fn type_is_struct",
        "pub fn type_id_as_enum",
        "pub fn inherited_id_as_enum",
        "pub fn is_id_as_enum",
        "pub fn id_as_enum_names",
        "pub fn ref_target_names",
        "fn collect_ref_targets_for_type",
        "fn annotation_name_arg",
    ] {
        assert!(
            queries.contains(expected),
            "CFT schema query helper `{expected}` should live in schema_view/queries.rs"
        );
        assert!(
            !schema_view.contains(expected),
            "CFT schema query helper `{expected}` should not bloat schema_view.rs"
        );
    }
}

#[test]
fn cft_parser_check_expression_parser_is_split_out() {
    let parser =
        std::fs::read_to_string("crates/coflow-cft/src/parser.rs").expect("read CFT parser");
    let annotations = std::fs::read_to_string("crates/coflow-cft/src/parser/annotations.rs")
        .expect("read annotation parser");
    let check = std::fs::read_to_string("crates/coflow-cft/src/parser/check.rs")
        .expect("read check parser");
    let check_primary = std::fs::read_to_string("crates/coflow-cft/src/parser/check_primary.rs")
        .expect("read check primary parser");
    let defaults = std::fs::read_to_string("crates/coflow-cft/src/parser/defaults.rs")
        .expect("read default parser");
    let definitions = std::fs::read_to_string("crates/coflow-cft/src/parser/definitions.rs")
        .expect("read definition parser");
    let literals = std::fs::read_to_string("crates/coflow-cft/src/parser/literals.rs")
        .expect("read literal parser");
    let tokens = std::fs::read_to_string("crates/coflow-cft/src/parser/tokens.rs")
        .expect("read parser tokens");

    for expected in [
        "pub(super) fn parse_check_block",
        "fn parse_check_stmts",
        "fn parse_or_expr",
        "fn validate_cmp_chain",
        "enum CmpChainGroup",
    ] {
        assert!(
            check.contains(expected),
            "CFT parser check helper `{expected}` should live in parser/check.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser check helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_postfix_expr",
        "fn parse_primary_expr",
        "pub(super) fn parse_type_predicate",
    ] {
        assert!(
            check_primary.contains(expected),
            "CFT parser primary check helper `{expected}` should live in parser/check_primary.rs"
        );
        assert!(
            !check.contains(expected),
            "CFT parser primary check helper `{expected}` should not live in parser/check.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser primary check helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_annotation",
        "fn parse_annotation_arg",
        "fn parse_annotation_arg_for",
    ] {
        assert!(
            annotations.contains(expected),
            "CFT parser annotation helper `{expected}` should live in parser/annotations.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser annotation helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_default_expr",
        "fn parse_negative_default",
        "fn parse_name_or_enum_default",
        "fn parse_array_default",
        "fn parse_object_default",
    ] {
        assert!(
            defaults.contains(expected),
            "CFT parser default helper `{expected}` should live in parser/defaults.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser default helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_const",
        "pub(super) fn parse_enum",
        "pub(super) fn parse_type",
        "fn parse_field",
        "pub(super) fn parse_type_ref",
        "fn parse_type_ref_primary",
    ] {
        assert!(
            definitions.contains(expected),
            "CFT parser definition helper `{expected}` should live in parser/definitions.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser definition helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_const_literal",
        "fn parse_negative_const_literal",
        "pub(super) fn parse_signed_int",
    ] {
        assert!(
            literals.contains(expected),
            "CFT parser literal helper `{expected}` should live in parser/literals.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser literal helper `{expected}` should not live in parser.rs"
        );
    }
    for expected in [
        "pub(super) fn token_name",
        "pub(super) fn reserved_keyword_name",
    ] {
        assert!(
            tokens.contains(expected),
            "CFT parser token helper `{expected}` should live in parser/tokens.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFT parser token helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 220,
        "coflow-cft parser.rs should stay focused on entrypoint and token cursor utilities"
    );
}

#[test]
fn csharp_codegen_schema_projection_uses_cft_schema_view() {
    let schema_view = std::fs::read_to_string("crates/coflow-codegen-csharp/src/schema_view.rs")
        .expect("read C# codegen schema view");
    let ir = std::fs::read_to_string("crates/coflow-codegen-csharp/src/ir.rs").expect("read C# IR");
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
        "pub struct TypeMeta",
        "pub struct FieldMeta",
        "pub enum FieldType",
        "children:",
        "fill_concrete_descendants",
        "pub enums:",
        "view.enums",
        ".types.get(",
        ".types.values()",
        ".types.keys()",
        ".enums.get(",
        ".enums.values()",
        ".enums.keys()",
        ".enums.contains_key(",
        "CftSchemaType,",
        "CftSchemaEnum,",
        "CftSchemaField,",
    ] {
        assert!(
            !codegen.contains(forbidden),
            "C# codegen should not rebuild schema projection from `{forbidden}`"
        );
    }
    assert!(
        schema_view.contains("pub type TypeMeta = CftTypeMeta;")
            && schema_view.contains("pub type FieldMeta = CftFieldMeta;"),
        "C# codegen should re-export coflow-cft type/field metadata instead of defining local copies"
    );
    for expected in [
        "self.cft.id_as_enum_names()",
        "self.cft.inherited_id_as_enum(type_name)",
        "self.cft.ref_target_names()",
        "self.cft.type_is_struct(&ty.name)",
    ] {
        assert!(
            schema_view.contains(expected),
            "C# codegen schema facade should delegate `{expected}` to coflow-cft"
        );
    }
    for forbidden in [
        "fn type_id_as_enum",
        "fn collect_ref_targets_for_type",
        "fn collect_ref_targets_in_field",
        "fn collect_ref_targets_in_type",
        "annotation_name_arg(&ty.annotations",
        "has_annotation(&ty.annotations",
    ] {
        assert!(
            !schema_view.contains(forbidden),
            "C# codegen schema facade should not keep local schema semantic helper `{forbidden}`"
        );
    }
    assert!(
        codegen.contains("CftSchemaTypeRef"),
        "C# codegen emit path should consume coflow-cft type refs directly"
    );
}

#[test]
fn cft_lexer_tokens_are_split_out() {
    let lexer = std::fs::read_to_string("crates/coflow-cft/src/lexer.rs").expect("read CFT lexer");
    let tokens = std::fs::read_to_string("crates/coflow-cft/src/lexer/tokens.rs")
        .expect("read CFT lexer tokens");

    for expected in ["pub enum TokenKind", "pub struct Token"] {
        assert!(
            tokens.contains(expected),
            "CFT lexer token item `{expected}` should live in lexer/tokens.rs"
        );
        assert!(
            !lexer.contains(expected),
            "CFT lexer token item `{expected}` should not live in lexer.rs"
        );
    }
    assert!(
        lexer.lines().count() < 450,
        "coflow-cft lexer.rs should stay below the 450-line focused-module threshold"
    );
}

#[test]
fn csharp_codegen_emit_type_helpers_do_not_live_in_emit_rs() {
    let emit =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");
    let identifiers =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/identifiers.rs")
            .expect("read C# emit identifiers");
    let types = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/types.rs")
        .expect("read C# emit types");

    for expected in [
        "pub(super) fn field_local_name",
        "pub(super) fn loader_reserved_local_names",
        "pub(super) fn plural_records_var",
        "pub(super) fn context_index_field_name",
    ] {
        assert!(
            identifiers.contains(expected),
            "C# emit identifier helper `{expected}` should live in emit/identifiers.rs"
        );
        assert!(
            !emit.contains(expected),
            "C# emit identifier helper `{expected}` should not live in emit.rs"
        );
    }
    for expected in [
        "pub(super) fn csharp_type",
        "pub(super) fn csharp_field_property_type",
        "pub(super) fn csharp_property_type",
        "pub(super) fn default_value_expr",
        "pub(super) fn collection_default_expr",
    ] {
        assert!(
            types.contains(expected),
            "C# emit type helper `{expected}` should live in emit/types.rs"
        );
        assert!(
            !emit.contains(expected),
            "C# emit type helper `{expected}` should not live in emit.rs"
        );
    }
}

#[test]
fn csharp_codegen_emit_database_helpers_do_not_live_in_emit_rs() {
    let emit =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");
    let database = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/database.rs")
        .expect("read C# emit database");

    for expected in [
        "pub fn build_csharp_database",
        "fn build_context_lookups",
        "fn build_json_load_steps",
        "fn build_messagepack_load_steps",
        "fn build_table_model",
        "fn sort_tables_by_dependencies",
        "fn collect_table_dependencies",
    ] {
        assert!(
            database.contains(expected),
            "C# emit database helper `{expected}` should live in emit/database.rs"
        );
        assert!(
            !emit.contains(expected),
            "C# emit database helper `{expected}` should not live in emit.rs"
        );
    }
}

#[test]
fn csharp_codegen_emit_reader_helpers_do_not_live_in_emit_rs() {
    let emit =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");
    let readers = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/readers.rs")
        .expect("read C# emit readers");

    for expected in [
        "pub(super) fn read_field_expr",
        "pub(super) fn read_token_expr",
        "pub(super) fn read_messagepack_field_expr",
        "pub(super) fn read_messagepack_expr",
        "fn read_dict_key_expr",
        "fn read_messagepack_dict_key_expr",
    ] {
        assert!(
            readers.contains(expected),
            "C# emit reader helper `{expected}` should live in emit/readers.rs"
        );
        assert!(
            !emit.contains(expected),
            "C# emit reader helper `{expected}` should not live in emit.rs"
        );
    }
}

#[test]
fn csharp_codegen_emit_loader_helpers_do_not_live_in_emit_rs() {
    let emit =
        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");
    let loaders = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/loaders.rs")
        .expect("read C# emit loaders");

    for expected in [
        "pub(super) fn loader_method",
        "pub(super) fn polymorphic_loader",
        "fn polymorphic_cases",
        "fn load_field",
        "pub(super) fn field_type_requires_context",
        "fn field_type_requires_context_inner",
    ] {
        assert!(
            loaders.contains(expected),
            "C# emit loader helper `{expected}` should live in emit/loaders.rs"
        );
        assert!(
            !emit.contains(expected),
            "C# emit loader helper `{expected}` should not live in emit.rs"
        );
    }
    assert!(
        emit.lines().count() < 320,
        "coflow-codegen-csharp emit.rs should stay below the 320-line focused-module threshold"
    );
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
        "struct SchemaView",
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
    for forbidden in ["schema.all_types()", "schema.resolve_type(", ".types.get("] {
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
        ".types.get(",
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
    for forbidden in [
        ".resolve_type(",
        ".all_fields",
        ".has_enum(",
        ".types.get(",
        ".types.contains_key(",
    ] {
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
    for forbidden in [
        ".resolve_type(",
        "session.schema.resolve_type",
        ".types.get(",
    ] {
        assert!(
            !data_files.contains(forbidden),
            "data file commands should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_writes_use_cft_schema_view_for_insert_schema_checks() {
    let writes =
        std::fs::read_to_string("crates/coflow-engine/src/writes.rs").expect("read engine writes");

    assert!(
        writes.contains("CftSchemaView::new(&session.schema)"),
        "engine writes should use CftSchemaView for insert schema checks"
    );
    for forbidden in [
        "session.schema.resolve_type",
        ".all_fields",
        ".types.get(",
        ".types.contains_key(",
    ] {
        assert!(
            !writes.contains(forbidden),
            "engine writes should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_write_path_helpers_do_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-engine/src/writes.rs").expect("read engine writes");
    let path = std::fs::read_to_string("crates/coflow-engine/src/writes/path.rs")
        .expect("read engine write path helpers");

    for expected in [
        "pub(super) fn write_path_from_cfd_path",
        "pub(super) fn value_at_path",
        "fn format_dict_key_for_path",
        "pub(super) fn cfd_path_from_write_path",
        "pub(super) fn cfd_path_to_write_path",
    ] {
        assert!(
            path.contains(expected),
            "engine write path helper `{expected}` should live in writes/path.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine write path helper `{expected}` should not live in writes.rs"
        );
    }
    assert!(
        writes.lines().count() < 800,
        "coflow-engine writes.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn engine_write_reference_planning_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-engine/src/writes.rs").expect("read engine writes");
    let refs = std::fs::read_to_string("crates/coflow-engine/src/writes/refs.rs")
        .expect("read engine write reference helpers");

    for expected in [
        "pub(super) struct ReferenceUpdateAction",
        "pub(super) struct OwnedWriteCellRequest",
        "pub(super) struct SourceRewriteAction",
        "pub(super) struct OwnedRewriteRecordReferencesRequest",
        "pub(super) fn reference_update_actions",
        "pub(super) fn source_rewrite_actions",
    ] {
        assert!(
            refs.contains(expected),
            "engine write reference planning item `{expected}` should live in writes/refs.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine write reference planning item `{expected}` should not live in writes.rs"
        );
    }
    assert!(
        writes.lines().count() < 560,
        "coflow-engine writes.rs should stay below the 560-line focused-module threshold"
    );
}

#[test]
fn engine_mutation_defaults_use_cft_schema_view() {
    let defaults = std::fs::read_to_string("crates/coflow-engine/src/mutation/defaults.rs")
        .expect("read mutation defaults")
        .replace("\r\n", "\n");

    assert!(
        defaults.contains("CftSchemaView::new(schema)"),
        "mutation default materialization should use coflow-cft CftSchemaView"
    );
    for forbidden in [
        ".resolve_type(",
        ".resolve_enum(",
        ".has_enum(",
        ".types.get(",
        ".types.contains_key(",
        ".enums.contains_key(",
        "CftSchemaField",
    ] {
        assert!(
            !defaults.contains(forbidden),
            "mutation default materialization should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_mutation_field_coercion_uses_cft_schema_view() {
    let coercion = std::fs::read_to_string("crates/coflow-engine/src/mutation/coercion.rs")
        .expect("read mutation coercion")
        .replace("\r\n", "\n");

    assert!(
        coercion.contains("CftSchemaView::new(&session.schema)"),
        "mutation field coercion should use coflow-cft CftSchemaView for field lookup"
    );
    for forbidden in [
        "CftSchemaField",
        "session.schema.resolve_type(",
        ".resolve_type(actual_type)",
        ".all_fields\n            .iter()",
    ] {
        assert!(
            !coercion.contains(forbidden),
            "mutation field coercion should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_mutation_uses_cft_schema_view_for_schema_queries() {
    let mutation =
        std::fs::read_to_string("crates/coflow-engine/src/mutation/mod.rs").expect("read mutation");
    let coercion = std::fs::read_to_string("crates/coflow-engine/src/mutation/coercion.rs")
        .expect("read mutation coercion");

    assert!(
        mutation.contains("CftSchemaView::new(&session.schema)")
            || mutation.contains("CftSchemaView::new(schema)")
            || coercion.contains("CftSchemaView::new(&session.schema)"),
        "mutation schema queries should go through coflow-cft CftSchemaView"
    );
    for forbidden in [
        ".resolve_type(",
        ".resolve_enum(",
        ".has_enum(",
        ".types.get(",
        ".types.contains_key(",
        ".enums.contains_key(",
        "CftSchemaField",
    ] {
        assert!(
            !mutation.contains(forbidden),
            "mutation should not use raw schema query `{forbidden}`"
        );
        assert!(
            !coercion.contains(forbidden),
            "mutation coercion should not use raw schema query `{forbidden}`"
        );
    }
}

#[test]
fn engine_public_mutation_api_does_not_expose_prepared_ops() {
    let engine =
        std::fs::read_to_string("crates/coflow-engine/src/lib.rs").expect("read engine source");
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation/types.rs")
        .expect("read mutation types")
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
fn engine_mutation_wire_types_do_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation/mod.rs")
        .expect("read mutation module");
    let types = std::fs::read_to_string("crates/coflow-engine/src/mutation/types.rs")
        .expect("read mutation types module");

    for expected in [
        "pub struct MutationRequest",
        "pub enum MutationOp",
        "pub enum MutationValue",
        "pub enum MutationFields",
        "pub struct MutationReport",
    ] {
        assert!(
            types.contains(expected),
            "mutation wire type `{expected}` should live in mutation/types.rs"
        );
        assert!(
            !mutation.contains(expected),
            "mutation wire type `{expected}` should not live in mutation/mod.rs"
        );
    }
}

#[test]
fn engine_mutation_defaults_do_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation/mod.rs")
        .expect("read mutation module");
    let defaults = std::fs::read_to_string("crates/coflow-engine/src/mutation/defaults.rs")
        .expect("read mutation defaults module");

    for expected in [
        "fn default_record_for_type",
        "fn default_missing_fields_for_type",
        "fn default_fields_for_type_inner",
        "fn default_from_schema_default",
    ] {
        assert!(
            defaults.contains(expected),
            "mutation default helper `{expected}` should live in mutation/defaults.rs"
        );
        assert!(
            !mutation.contains(expected),
            "mutation default helper `{expected}` should not live in mutation/mod.rs"
        );
    }
}

#[test]
fn engine_mutation_coercion_does_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-engine/src/mutation/mod.rs")
        .expect("read mutation module");
    let coercion = std::fs::read_to_string("crates/coflow-engine/src/mutation/coercion.rs")
        .expect("read mutation coercion module");

    for expected in [
        "fn coerce_mutation_value",
        "fn coerce_json_field_value",
        "fn coerce_cfd_field_value",
        "fn coerce_json_value",
        "fn coerce_cfd_value",
        "fn validate_value_for_write",
    ] {
        assert!(
            coercion.contains(expected),
            "mutation coercion helper `{expected}` should live in mutation/coercion.rs"
        );
        assert!(
            !mutation.contains(expected),
            "mutation coercion helper `{expected}` should not live in mutation/mod.rs"
        );
    }
}

#[test]
fn checker_diagnostic_rendering_does_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let diagnostics = std::fs::read_to_string("crates/coflow-checker/src/check/diagnostics.rs")
        .expect("read checker diagnostics");

    for expected in [
        "pub(super) struct CheckExplanation",
        "pub(super) fn render_stmt",
        "pub(super) fn render_expr",
        "pub(super) fn format_value_for_message",
        "pub(super) fn dimension_lookup_error_message",
    ] {
        assert!(
            diagnostics.contains(expected),
            "checker diagnostic helper `{expected}` should live in check/diagnostics.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker diagnostic helper `{expected}` should not live in evaluator.rs"
        );
    }
}

#[test]
fn checker_numeric_ops_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let ops = std::fs::read_to_string("crates/coflow-checker/src/check/ops.rs")
        .expect("read checker ops");

    for expected in [
        "pub(super) fn checked_int",
        "pub(super) fn checked_shift",
        "pub(super) fn unary_op",
        "pub(super) fn compare",
        "pub(super) fn compare_order",
        "pub(super) fn eager_bin_op",
        "pub(super) fn expect_bool_operand",
    ] {
        assert!(
            ops.contains(expected),
            "checker operation helper `{expected}` should live in check/ops.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker operation helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in [
        "整数取负溢出",
        "不支持的一元运算",
        "整数加法溢出",
        "不支持的二元运算",
        "操作数不是 bool",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own eager binary op branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_builtin_value_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let builtin_values =
        std::fs::read_to_string("crates/coflow-checker/src/check/builtin_values.rs")
            .expect("read checker builtin values");

    for expected in [
        "pub(super) fn len_value",
        "pub(super) fn contains_value",
        "pub(super) fn unique_value",
        "pub(super) fn keys_value",
        "pub(super) fn values_value",
        "pub(super) fn matches_value",
        "pub(super) fn min_max_value",
        "pub(super) fn sum_value",
    ] {
        assert!(
            builtin_values.contains(expected),
            "checker builtin value helper `{expected}` should live in check/builtin_values.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker builtin value helper `{expected}` should not live in evaluator.rs"
        );
    }
}

#[test]
fn checker_builtin_call_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let builtin_calls = std::fs::read_to_string("crates/coflow-checker/src/check/builtin_calls.rs")
        .expect("read checker builtin call helpers");

    for expected in [
        "pub(super) struct CallSignature",
        "pub(super) enum CallTarget",
        "pub(super) enum CallSignatureError",
        "pub(super) fn matches_pattern_arg",
        "fn require_arity",
    ] {
        assert!(
            builtin_calls.contains(expected),
            "checker builtin call helper `{expected}` should live in check/builtin_calls.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker builtin call helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in [
        "枚举构造函数需要 1 个参数",
        "matches 的 pattern 必须是字符串字面量",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own builtin call validation branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_dependency_collection_does_not_live_in_evaluator_or_runner_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let runner =
        std::fs::read_to_string("crates/coflow-checker/src/check/runner.rs").expect("read runner");
    let deps =
        std::fs::read_to_string("crates/coflow-checker/src/check/deps.rs").expect("read deps");

    for expected in [
        "pub(super) struct DependencyCollector",
        "pub(super) struct DependencyGraphBuilder",
    ] {
        assert!(
            deps.contains(expected),
            "checker dependency helper `{expected}` should live in check/deps.rs"
        );
        assert!(
            !evaluator.contains(expected) && !runner.contains(expected),
            "checker dependency helper `{expected}` should not live in evaluator.rs or runner.rs"
        );
    }
}

#[test]
fn checker_quantifier_item_expansion_does_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let quantifiers = std::fs::read_to_string("crates/coflow-checker/src/check/quantifiers.rs")
        .expect("read checker quantifier helpers");

    assert!(
        quantifiers.contains("pub(super) fn quantifier_items"),
        "checker quantifier item expansion should live in check/quantifiers.rs"
    );
    assert!(
        !evaluator.contains("fn quantifier_items"),
        "checker quantifier item expansion should not live in evaluator.rs"
    );
    for forbidden in ["量词目标不是集合", "format_check_key_for_path"] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own quantifier item expansion branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_statement_execution_does_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let statements = std::fs::read_to_string("crates/coflow-checker/src/check/statements.rs")
        .expect("read checker statements");

    for expected in [
        "pub(super) fn eval_check_block",
        "fn eval_stmts",
        "fn eval_stmt",
        "fn eval_quantifier_stmt",
        "fn rewrite_all_failures",
        "fn emit_any_failure",
        "fn emit_none_failures",
    ] {
        assert!(
            statements.contains(expected),
            "checker statement helper `{expected}` should live in check/statements.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker statement helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in [
        "check 表达式没有求值为 bool",
        "when 条件没有求值为 bool",
        "CheckAllQuantifierFailed",
        "CheckAnyQuantifierFailed",
        "CheckNoneQuantifierFailed",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own statement execution branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_expression_dispatch_does_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let expressions = std::fs::read_to_string("crates/coflow-checker/src/check/expressions.rs")
        .expect("read checker expressions");

    for expected in [
        "pub(super) fn eval_expr",
        "fn eval_field_expr",
        "fn eval_index_expr",
        "fn eval_is_expr",
        "fn eval_cmp_chain_expr",
    ] {
        assert!(
            expressions.contains(expected),
            "checker expression helper `{expected}` should live in check/expressions.rs"
        );
    }
    for forbidden in [
        "CftSchemaCheckExprKind::Int",
        "CftSchemaCheckExprKind::Field",
        "CftSchemaCheckExprKind::Index",
        "CftSchemaCheckExprKind::CmpChain",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own expression dispatch branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_field_read_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let fields = std::fs::read_to_string("crates/coflow-checker/src/check/fields.rs")
        .expect("read checker field helpers");

    for expected in [
        "pub(super) fn field_type_for_record",
        "pub(super) fn current_field",
        "pub(super) fn field_value",
        "pub(super) fn virtual_id",
    ] {
        assert!(
            fields.contains(expected),
            "checker field helper `{expected}` should live in check/fields.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker field helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in [
        "不能访问 null 的字段",
        "记录没有字段",
        "字段访问目标不是对象",
        "dict entry 没有字段",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own field read branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_value_explanation_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let explanations = std::fs::read_to_string("crates/coflow-checker/src/check/explanations.rs")
        .expect("read checker explanation helpers");

    for expected in [
        "pub(super) trait ValueExprEvaluator",
        "pub(super) fn explain_false_value_expr",
        "pub(super) fn value_expr_actual",
        "fn unique_failed_explanation",
    ] {
        assert!(
            explanations.contains(expected),
            "checker explanation helper `{expected}` should live in check/explanations.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker explanation helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in ["所有元素唯一", "重复值", "包含 {}", "匹配 {}"] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own value explanation branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_explained_eval_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let explanations = std::fs::read_to_string("crates/coflow-checker/src/check/explanations.rs")
        .expect("read checker explanation helpers");

    assert!(
        explanations.contains("pub(super) fn eval_expr_explained"),
        "checker explained expression evaluation should live in check/explanations.rs"
    );
    assert!(
        !evaluator.contains("fn eval_expr_explained"),
        "checker explained expression evaluation body should not live in evaluator.rs"
    );
    for forbidden in ["左侧条件为 false", "右侧条件为 false", "期望 !"] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own explained eval branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_false_explanation_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let explanations = std::fs::read_to_string("crates/coflow-checker/src/check/explanations.rs")
        .expect("read checker explanation helpers");

    for expected in [
        "pub(super) fn explain_false_expr",
        "fn explain_failed_comparison",
    ] {
        assert!(
            explanations.contains(expected),
            "checker false explanation helper `{expected}` should live in check/explanations.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker false explanation helper `{expected}` should not live in evaluator.rs"
        );
    }
    for forbidden in [
        "至少一个操作数为 false",
        "两侧都为 true",
        "至少一侧为 true",
        "实际类型 =",
    ] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own false explanation branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_type_predicate_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let type_predicates =
        std::fs::read_to_string("crates/coflow-checker/src/check/type_predicates.rs")
            .expect("read checker type predicate helpers");

    assert!(
        type_predicates.contains("pub(super) fn value_matches_predicate"),
        "checker type predicate helper should live in check/type_predicates.rs"
    );
    assert!(
        !evaluator.contains("fn eval_is") && !evaluator.contains("fn value_matches_predicate"),
        "checker type predicate helper should not live in evaluator.rs"
    );
    assert!(
        !evaluator.contains("schema.is_assignable(actual, type_name)"),
        "checker evaluator should not own type predicate assignability checks"
    );
}

#[test]
fn checker_dimension_variant_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let dimensions = std::fs::read_to_string("crates/coflow-checker/src/check/dimensions.rs")
        .expect("read checker dimension helpers");

    assert!(
        dimensions.contains("pub(super) fn apply_dimension_variant"),
        "checker dimension variant helper should live in check/dimensions.rs"
    );
    assert!(
        !evaluator.contains("dimension_field_value(")
            && !evaluator.contains("dimension_lookup_error_message"),
        "checker evaluator should not own dimension variant lookup details"
    );
    assert!(
        evaluator.lines().count() < 800,
        "checker evaluator.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn checker_index_access_does_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let access =
        std::fs::read_to_string("crates/coflow-checker/src/check/access.rs").expect("read access");

    assert!(
        access.contains("pub(super) fn index_value"),
        "checker index access should live in check/access.rs"
    );
    assert!(
        !evaluator.contains("pub(super) fn index_value"),
        "checker index access should not live in evaluator.rs"
    );
    for forbidden in ["CheckMissingDictKey", "数组索引越界", "索引目标不是集合"] {
        assert!(
            !evaluator.contains(forbidden),
            "checker evaluator should not own index access branch `{forbidden}`"
        );
    }
}

#[test]
fn checker_enum_value_helpers_do_not_live_in_evaluator_rs() {
    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")
        .expect("read checker evaluator");
    let enum_values = std::fs::read_to_string("crates/coflow-checker/src/check/enum_values.rs")
        .expect("read checker enum values");

    for expected in [
        "pub(super) fn enum_with_value",
        "pub(super) fn anonymous_enum_value",
    ] {
        assert!(
            enum_values.contains(expected),
            "checker enum value helper `{expected}` should live in check/enum_values.rs"
        );
        assert!(
            !evaluator.contains(expected),
            "checker enum value helper `{expected}` should not live in evaluator.rs"
        );
    }
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
fn lsp_protocol_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let protocol =
        std::fs::read_to_string("crates/coflow-lsp/src/protocol.rs").expect("read lsp protocol");

    for expected in [
        "pub(crate) struct TextRequest",
        "pub(crate) fn read_message",
        "pub(crate) fn did_open_document",
        "pub(crate) fn text_document_uri",
    ] {
        assert!(
            protocol.contains(expected),
            "LSP protocol helper `{expected}` should live in protocol.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP protocol helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_state_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let state = std::fs::read_to_string("crates/coflow-lsp/src/state.rs").expect("read lsp state");

    for expected in [
        "pub(crate) struct LspBuild",
        "pub(crate) struct LspDocument",
        "pub(crate) fn current_type_at",
        "pub(crate) fn current_field_at",
        "pub(crate) fn type_of_chain",
        "pub(crate) fn field_by_type",
        "pub(crate) fn field_by_chain",
        "pub(crate) fn enum_variant_by_chain",
        "pub(crate) fn enum_name_exists",
        "pub(crate) fn enum_variant_exists",
        "pub(crate) fn quantifier_bindings_at",
        "fn collect_quantifier_bindings",
    ] {
        assert!(
            state.contains(expected),
            "LSP state helper `{expected}` should live in state.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP state helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_text_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let text = std::fs::read_to_string("crates/coflow-lsp/src/text.rs").expect("read lsp text");

    for expected in [
        "pub(crate) struct WordAt",
        "pub(crate) fn is_trivia_position",
        "pub(crate) fn is_inside_string",
        "pub(crate) fn dotted_chain_at",
        "pub(crate) fn word_at",
        "pub(crate) fn parse_dotted_ident_chain",
        "pub(crate) fn previous_char",
        "pub(crate) fn is_ident_continue",
        "pub(crate) fn last_ident",
        "pub(crate) fn line_prefix_at",
        "pub(crate) fn is_after_line_comment",
    ] {
        assert!(
            text.contains(expected),
            "LSP text helper `{expected}` should live in text.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP text helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_position_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let position =
        std::fs::read_to_string("crates/coflow-lsp/src/position.rs").expect("read lsp position");

    for expected in [
        "pub(crate) struct LspPosition",
        "pub(crate) fn full_document_range",
        "pub(crate) fn byte_range",
        "pub(crate) fn range_from_span",
        "pub(crate) fn byte_offset_from_position",
        "pub(crate) fn position_from_byte",
    ] {
        assert!(
            position.contains(expected),
            "LSP position helper `{expected}` should live in position.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP position helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_formatting_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let formatting = std::fs::read_to_string("crates/coflow-lsp/src/formatting.rs")
        .expect("read lsp formatting");

    for expected in [
        "pub(crate) fn format_cft",
        "pub(crate) fn starts_with_closing_delimiter",
        "pub(crate) fn adjusted_indent",
    ] {
        assert!(
            formatting.contains(expected),
            "LSP formatting helper `{expected}` should live in formatting.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP formatting helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_document_symbol_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let document_symbols = std::fs::read_to_string("crates/coflow-lsp/src/document_symbols.rs")
        .expect("read lsp document symbols");

    for expected in [
        "pub(crate) fn document_symbols",
        "fn document_symbol_item",
        "const SYMBOL_KIND_CLASS",
        "const SYMBOL_KIND_ENUM_MEMBER",
    ] {
        assert!(
            document_symbols.contains(expected),
            "LSP document symbol helper `{expected}` should live in document_symbols.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP document symbol helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_definition_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let definition =
        std::fs::read_to_string("crates/coflow-lsp/src/definition.rs").expect("read definition");

    for expected in [
        "pub(crate) fn definitions_at",
        "pub(crate) fn cft_type_definition_location",
        "pub(crate) fn cft_schema_field_definition_location",
        "pub(crate) fn cfd_record_definition_location",
        "pub(crate) fn field_location_by_chain",
        "pub(crate) fn field_location",
        "pub(crate) fn ast_enum_variant_location",
        "fn cfd_record_definition_location_in_source",
        "fn cfd_project_sources",
        "fn cfd_sources_in_dir",
        "fn cfd_source_from_path",
    ] {
        assert!(
            definition.contains(expected),
            "LSP definition helper `{expected}` should live in definition.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP definition helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_documentation_catalog_does_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let documentation = std::fs::read_to_string("crates/coflow-lsp/src/documentation.rs")
        .expect("read lsp documentation");

    for expected in [
        "pub(crate) const KEYWORDS",
        "pub(crate) const PRIMITIVE_TYPES",
        "pub(crate) const LITERALS",
        "pub(crate) const BUILTIN_FUNCTIONS",
        "pub(crate) const ANNOTATIONS",
        "pub(crate) struct AnnotationCompletion",
        "pub(crate) fn static_documentation",
        "pub(crate) fn is_builtin_name",
    ] {
        assert!(
            documentation.contains(expected),
            "LSP documentation catalog `{expected}` should live in documentation.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP documentation catalog `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_completion_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let completion = std::fs::read_to_string("crates/coflow-lsp/src/completion.rs")
        .expect("read lsp completion");

    for expected in [
        "pub(crate) fn completion_items",
        "pub(crate) enum CompletionScope",
        "pub(crate) fn completion_scope",
        "fn completion_item",
        "fn annotation_completion_item",
        "pub(crate) fn annotation_completion_items",
        "pub(crate) fn top_level_completion_items",
        "pub(crate) fn check_expression_completion_items",
        "pub(crate) fn dot_completion_items",
        "fn const_value_assignable_to_type",
        "pub(crate) fn is_annotation_completion_context",
        "pub(crate) fn is_type_predicate_context",
        "pub(crate) fn is_type_header_parent_context",
        "pub(crate) fn is_type_reference_context",
        "pub(crate) fn is_const_value_context",
        "pub(crate) fn is_field_default_context",
        "pub(crate) fn top_level_needs_type_keyword",
        "pub(crate) fn receiver_chain_before_dot",
        "fn trailing_dotted_ident_chain",
    ] {
        assert!(
            completion.contains(expected),
            "LSP completion helper `{expected}` should live in completion.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP completion helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_hover_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let hover = std::fs::read_to_string("crates/coflow-lsp/src/hover.rs").expect("read lsp hover");

    for expected in [
        "pub(crate) fn hover_at",
        "fn type_hover_text",
        "fn const_value_to_string",
        "fn hover_response",
        "fn annotation_at",
    ] {
        assert!(
            hover.contains(expected),
            "LSP hover helper `{expected}` should live in hover.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP hover helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lsp_semantic_token_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");
    let semantic_tokens = std::fs::read_to_string("crates/coflow-lsp/src/semantic_tokens.rs")
        .expect("read lsp semantic tokens");

    for expected in [
        "pub(crate) const SEMANTIC_TOKEN_TYPES",
        "pub(crate) const SEMANTIC_TOKEN_MODIFIERS",
        "pub(crate) struct RawSemanticToken",
        "pub(crate) fn push_semantic_span",
        "pub(crate) fn push_semantic_span_plain",
        "pub(crate) fn encode_semantic_tokens",
        "pub(crate) fn add_comment_semantic_tokens",
        "pub(crate) fn comment_start_in_line",
        "pub(crate) fn semantic_token_data",
        "fn semantic_raw_tokens",
        "fn add_lex_semantic_token",
        "fn add_ast_semantic_tokens",
        "fn add_annotation_semantic",
        "fn add_type_ref_semantic",
        "fn add_const_literal_semantic",
        "fn add_default_expr_semantic",
        "fn add_check_stmt_semantic",
        "fn add_check_expr_semantic",
    ] {
        assert!(
            semantic_tokens.contains(expected),
            "LSP semantic token helper `{expected}` should live in semantic_tokens.rs"
        );
        assert!(
            !lib.contains(expected),
            "LSP semantic token helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_dto_types_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let dto =
        std::fs::read_to_string("crates/coflow-loader-lark/src/dto.rs").expect("read lark dto");

    for expected in [
        "pub(crate) struct AuthResponse",
        "pub(crate) struct ApiEnvelope",
        "pub(crate) struct WikiNodeData",
        "pub(crate) struct SheetsQueryData",
        "pub(crate) struct LarkSheetMetadata",
        "pub(crate) struct ValuesData",
        "pub(crate) struct ValueRange",
    ] {
        assert!(
            dto.contains(expected),
            "Lark loader DTO `{expected}` should live in dto.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark loader DTO `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_diagnostics_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-lark/src/diagnostics.rs")
        .expect("read lark diagnostics");

    for expected in [
        "pub struct LarkDiagnostics",
        "pub struct LarkDiagnostic",
        "pub(crate) fn lark_diagnostics_to_api",
        "pub(crate) fn table_diagnostics_to_api",
        "pub(crate) fn table_write_diagnostics_to_api",
        "pub(crate) fn lark_render_error",
    ] {
        assert!(
            diagnostics.contains(expected),
            "Lark diagnostics helper `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark diagnostics helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_source_parsing_does_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let source = std::fs::read_to_string("crates/coflow-loader-lark/src/source.rs")
        .expect("read lark source");

    for expected in [
        "pub struct LarkSheetSource",
        "pub enum LarkSheetLocator",
        "pub(crate) fn lark_source_from_spec",
        "fn table_sheet_config_from_value",
        "pub(crate) fn sheet_config_from_options",
        "pub(crate) fn lark_document",
        "pub(crate) fn lark_document_spreadsheet_token",
    ] {
        assert!(
            source.contains(expected),
            "Lark source helper `{expected}` should live in source.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark source helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_http_client_does_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let http =
        std::fs::read_to_string("crates/coflow-loader-lark/src/http.rs").expect("read lark http");

    for expected in [
        "pub trait LarkHttpClient",
        "pub struct UreqLarkHttpClient",
        "impl LarkHttpClient for UreqLarkHttpClient",
        "fn ureq_error_message",
    ] {
        assert!(
            http.contains(expected),
            "Lark HTTP helper `{expected}` should live in http.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark HTTP helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_load_pipeline_does_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let load =
        std::fs::read_to_string("crates/coflow-loader-lark/src/load.rs").expect("read lark load");

    for expected in [
        "pub fn load_lark_table_source",
        "pub fn load_lark_table_source_with_client",
        "pub struct LarkSheetLoader",
        "pub const LARK_SHEET_LOADER_DESCRIPTOR",
        "fn tenant_access_token",
        "fn spreadsheet_metadata",
        "fn sheet_values",
    ] {
        assert!(
            load.contains(expected),
            "Lark load helper `{expected}` should live in load.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark load helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_writer_cache_does_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let writer_cache = std::fs::read_to_string("crates/coflow-loader-lark/src/writer_cache.rs")
        .expect("read lark writer cache");

    for expected in [
        "pub(crate) struct LarkWriterCache",
        "struct CachedToken",
        "pub(crate) struct LarkWriteAuth",
        "pub(crate) fn cached_tenant_token",
        "pub(crate) fn cached_sheet_id",
        "pub(crate) fn invalidate_caches",
        "pub(crate) fn lark_write_auth",
        "fn lark_tenant_token_with_ttl",
        "pub(crate) fn fetch_sheet_id_map",
    ] {
        assert!(
            writer_cache.contains(expected),
            "Lark writer cache helper `{expected}` should live in writer_cache.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark writer cache helper `{expected}` should not live in lib.rs"
        );
    }
}

#[test]
fn lark_loader_write_operations_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")
        .expect("read lark loader lib");
    let write =
        std::fs::read_to_string("crates/coflow-loader-lark/src/write.rs").expect("read lark write");
    let write_http = std::fs::read_to_string("crates/coflow-loader-lark/src/write_http.rs")
        .expect("read lark write http");
    let write_layout = std::fs::read_to_string("crates/coflow-loader-lark/src/write_layout.rs")
        .expect("read lark write layout");

    for expected in [
        "impl<C> SourceWriter for LarkSheetWriter<C>",
        "pub static LARK_SHEET_WRITER_DESCRIPTOR",
    ] {
        assert!(
            write.contains(expected),
            "Lark writer operation `{expected}` should live in write.rs"
        );
        assert!(
            !lib.contains(expected),
            "Lark writer operation `{expected}` should not live in lib.rs"
        );
    }

    for expected in [
        "fn append_lark_row",
        "fn create_lark_sheet",
        "fn write_lark_header",
        "fn delete_lark_row",
        "fn read_lark_cell",
        "fn read_lark_header",
        "fn send_lark_write",
        "fn send_values_batch_update",
        "fn parse_write_envelope",
        "enum LarkWriteFailure",
        "enum LarkHttpMethod",
    ] {
        assert!(
            write_http.contains(expected),
            "Lark HTTP write helper `{expected}` should live in write_http.rs"
        );
        assert!(
            !lib.contains(expected) && !write.contains(expected),
            "Lark HTTP write helper `{expected}` should not live in lib.rs or write.rs"
        );
    }

    for expected in [
        "struct LarkInsertLayoutRequest",
        "fn lark_insert_layout",
        "fn resolve_lark_column",
    ] {
        assert!(
            write_layout.contains(expected),
            "Lark write layout helper `{expected}` should live in write_layout.rs"
        );
        assert!(
            !lib.contains(expected) && !write.contains(expected),
            "Lark write layout helper `{expected}` should not live in lib.rs or write.rs"
        );
    }
}

#[test]
fn table_writers_use_shared_cell_renderer() {
    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")
        .expect("read excel writer");
    let table_writer = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer.rs")
        .expect("read table writer");
    let table_writer_cells =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/cells.rs")
            .expect("read table writer cells");
    let lark = std::fs::read_to_string("crates/coflow-loader-lark/src/write.rs")
        .expect("read lark writer");

    assert!(
        excel.contains("coflow_loader_table_core::writer::{")
            && table_writer.contains("mod cells;")
            && table_writer_cells.contains("use crate::cell_value::render_cell_value;")
            && table_writer_cells.contains("render_cell_value(value).map_err(table_render_error)"),
        "Excel writer should use the shared table-core writer and renderer"
    );
    assert!(
        lark.contains("render_cell_value(request.new_value)"),
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
fn table_core_writer_diagnostics_do_not_live_in_writer_rs() {
    let writer = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer.rs")
        .expect("read table core writer");
    let cells = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/cells.rs")
        .expect("read table core writer cell rendering");
    let diagnostics =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/diagnostics.rs")
            .expect("read table core writer diagnostics");

    for expected in [
        "pub struct TableWriteDiagnostics",
        "pub struct TableWriteDiagnostic",
        "pub(super) fn one_error",
        "pub(super) fn table_render_error",
    ] {
        assert!(
            diagnostics.contains(expected),
            "table writer diagnostic item `{expected}` should live in writer/diagnostics.rs"
        );
        assert!(
            !writer.contains(expected),
            "table writer diagnostic item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.lines().count() < 330,
        "coflow-loader-table-core writer.rs should stay below the 450-line focused-module threshold"
    );
    for expected in [
        "pub(super) fn render_insert_value",
        "pub(super) fn render_field_cells",
        "fn root_value_for_path",
        "fn replace_subvalue",
        "fn resolve_column",
        "fn direct_child_columns",
        "fn format_dict_key_for_path",
    ] {
        assert!(
            cells.contains(expected),
            "table writer cell item `{expected}` should live in writer/cells.rs"
        );
        assert!(
            !writer.contains(expected),
            "table writer cell item `{expected}` should not live in writer.rs"
        );
    }
}

#[test]
fn table_loader_core_table_rs_is_split_by_responsibility() {
    let table = std::fs::read_to_string("crates/coflow-loader-table-core/src/table.rs")
        .expect("read table core loader");
    let types = std::fs::read_to_string("crates/coflow-loader-table-core/src/table/types.rs")
        .expect("read table core types");
    let diagnostics =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/table/diagnostics.rs")
            .expect("read table core diagnostics");
    let columns = std::fs::read_to_string("crates/coflow-loader-table-core/src/table/columns.rs")
        .expect("read table core columns");

    for expected in [
        "pub struct TableSheetConfig",
        "pub struct TableSource",
        "pub struct TableDiagnostic",
        "pub struct TableLocation",
    ] {
        assert!(
            types.contains(expected),
            "table type `{expected}` should live in table/types.rs"
        );
        assert!(
            !table.contains(expected),
            "table type `{expected}` should not live in table.rs"
        );
    }
    for expected in [
        "pub(super) enum TableLoadError",
        "pub(super) fn table_load_error_diagnostics",
    ] {
        assert!(
            diagnostics.contains(expected),
            "table diagnostic item `{expected}` should live in table/diagnostics.rs"
        );
        assert!(
            !table.contains(expected),
            "table diagnostic item `{expected}` should not live in table.rs"
        );
    }
    for expected in [
        "struct ColumnResolution",
        "pub(super) fn resolve_columns",
        "pub(super) fn field_columns_from_resolved",
    ] {
        assert!(
            columns.contains(expected),
            "table column item `{expected}` should live in table/columns.rs"
        );
        assert!(
            !table.contains(expected),
            "table column item `{expected}` should not live in table.rs"
        );
    }
    assert!(
        table.lines().count() < 800,
        "coflow-loader-table-core table.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn table_cell_value_is_split_by_responsibility() {
    let cell_value =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/mod.rs")
            .expect("read table core cell value parser");
    let collections =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/collections.rs")
            .expect("read table core cell value collections");
    let diagnostics =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/diagnostics.rs")
            .expect("read table core cell value diagnostics");
    let markers =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/markers.rs")
            .expect("read table core cell value markers");
    let objects =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/objects.rs")
            .expect("read table core cell value objects");
    let refs = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/refs.rs")
        .expect("read table core cell value refs");
    let render =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/render.rs")
            .expect("read table core cell value renderer");
    let scan = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/scan.rs")
        .expect("read table core cell value scanner");
    let strings =
        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/strings.rs")
            .expect("read table core cell value strings");
    let types = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/types.rs")
        .expect("read table core cell value type parser");

    for expected in [
        "pub struct CellValueDiagnostics",
        "pub struct CellValueDiagnostic",
        "pub enum CellValueErrorCode",
        "pub(super) fn syntax",
        "pub(super) fn type_mismatch",
    ] {
        assert!(
            diagnostics.contains(expected),
            "cell value diagnostic item `{expected}` should live in cell_value/diagnostics.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value diagnostic item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub enum CellRenderError",
        "pub fn render_cell_value",
        "fn render_array",
        "fn render_dict",
        "pub(super) fn render_string",
    ] {
        assert!(
            render.contains(expected),
            "cell value render item `{expected}` should live in cell_value/render.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value render item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) fn split_top_level",
        "pub(super) fn find_top_level_char",
        "pub(super) fn strip_outer_pair",
        "pub(super) fn find_marker_open_brace",
        "struct ScanState",
    ] {
        assert!(
            scan.contains(expected),
            "cell value scanner item `{expected}` should live in cell_value/scan.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value scanner item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) enum CellType",
        "struct TypeParser",
        "pub(super) struct FieldMeta",
        "pub(super) fn full_fields",
        "fn field_meta",
    ] {
        assert!(
            types.contains(expected),
            "cell value type item `{expected}` should live in cell_value/types.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value type item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_string",
        "pub(super) fn string_needs_quotes",
    ] {
        assert!(
            strings.contains(expected),
            "cell value string item `{expected}` should live in cell_value/strings.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value string item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) fn looks_like_bare_record_key",
        "pub(super) fn is_type_marker_name",
    ] {
        assert!(
            markers.contains(expected),
            "cell value marker item `{expected}` should live in cell_value/markers.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value marker item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in ["pub(super) fn parse_ref"] {
        assert!(
            refs.contains(expected),
            "cell value ref item `{expected}` should live in cell_value/refs.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value ref item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_object",
        "fn validate_actual_type",
        "fn parse_named_object",
        "fn parse_positional_object",
        "fn object_value",
        "struct ObjectContent",
        "fn object_content",
    ] {
        assert!(
            objects.contains(expected),
            "cell value object item `{expected}` should live in cell_value/objects.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value object item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    for expected in [
        "pub(super) fn parse_array",
        "fn reject_comma_array_item",
        "pub(super) fn parse_dict",
        "fn parse_dict_key",
    ] {
        assert!(
            collections.contains(expected),
            "cell value collection item `{expected}` should live in cell_value/collections.rs"
        );
        assert!(
            !cell_value.contains(expected),
            "cell value collection item `{expected}` should not live in cell_value/mod.rs"
        );
    }
    assert!(
        cell_value.lines().count() < 800,
        "coflow-loader-table-core cell_value/mod.rs should stay below the 800-line large-module threshold"
    );
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
