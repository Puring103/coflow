use super::*;

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

    assert!(

        id_as_enum.contains("use coflow_cft::{CftAnnotation, CftAnnotationValue, CftSchemaView};")

            && id_as_enum.contains("schema: &CftSchemaView")

            && id_as_enum.contains("schema.type_metas()")

            && id_as_enum.contains(".enum_meta(&enum_name)"),

        "root CLI @idAsEnum helper should use CftSchemaView"

    );

    for forbidden in [

        "CftContainer",

        "schema.all_types()",

        "schema.all_enums()",

        "schema.resolve_enum(",

        "schema.resolve_type(",

    ] {

        assert!(

            !id_as_enum.contains(forbidden),

            "root CLI @idAsEnum helper should not use raw schema owner query `{forbidden}`"

        );

    }

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

    assert!(

        artifacts.contains("use coflow_cft::CftSchemaView;")

            && artifacts.contains("pub schema: &'a CftSchemaView")

            && artifacts.contains("schema: &CftSchemaView"),

        "root CLI artifact helpers should receive the schema query facade"

    );

    for forbidden in ["CftContainer", "CftSchemaView::new("] {

        assert!(

            !artifacts.contains(forbidden),

            "root CLI artifact helpers should not own full schema container access `{forbidden}`"

        );

    }

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

        "pub(super) fn sync_lark_header",

        "fn lark_table_manager",

        "fn configured_lark_source",

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

    assert!(

        !output.contains("fn flat_diagnostics"),

        "data command output should use DiagnosticSet::flat_diagnostics instead of owning flat conversion"

    );

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

    assert!(

        lark.contains("table_header_layout("),

        "root CLI Lark helper should reuse runtime table header planning"

    );

    for forbidden in [

        "serde_json::Value",

        "TableSourceOptions",

        "TableSheetConfig",

        "options().get(\"sheets\")",

        ".as_array()",

        "Value::as_object",

        "filter_map(serde_json::Value::as_object)",

        "CftSchemaView::new(session.schema())",

        "session.schema_view()",

        "fn lark_table_layout",

        "fn lark_table_options",

        "fn matching_lark_sheet_config",

        "resolve_type(&actual_type)",

        "schema_type.all_fields",

    ] {

        assert!(

            !lark.contains(forbidden),

            "root CLI Lark helper should not bypass provider/schema facades with `{forbidden}`"

        );

    }

}



#[test]

fn root_cli_session_builders_use_runtime_facade() {

    let command_modules = [

        (

            "src/commands.rs",

            std::fs::read_to_string("src/commands.rs").expect("read root CLI commands"),

        ),

        (

            "src/data_commands.rs",

            std::fs::read_to_string("src/data_commands.rs").expect("read root CLI data commands"),

        ),

        (

            "src/schema_commands.rs",

            std::fs::read_to_string("src/schema_commands.rs")

                .expect("read root CLI schema commands"),

        ),

    ];



    for (path, source) in command_modules {

        assert!(

            source.contains("Runtime"),

            "{path} should construct project sessions through the runtime facade"

        );

        for forbidden in [

            "build_project_schema_session",

            "build_project_session_for_build",

            "open_project_session_read_only",

        ] {

            assert!(

                !source.contains(forbidden),

                "{path} should not call runtime session builder `{forbidden}` directly"

            );

        }

    }

}



#[test]

fn root_cli_schema_queries_go_through_session_facade() {

    let commands = std::fs::read_to_string("src/commands.rs").expect("read root CLI commands");

    let lark = std::fs::read_to_string("src/data_commands/lark.rs")

        .expect("read root CLI Lark data command helpers");



    let session = std::fs::read_to_string("crates/coflow-runtime/src/session.rs")

        .expect("read runtime session");

    assert!(

        session.contains("pub const fn schema_view(&self) -> &CftSchemaView")
            && session.contains("pub(crate) schema_view: CftSchemaView"),

        "runtime session should retain and lend the schema query facade"

    );



    assert!(

        commands.contains("session.schema_view()"),

        "root CLI build/export/codegen commands should request schema queries through the runtime session facade"

    );

    assert!(

        !commands.contains("CftSchemaView::new(session.schema())"),

        "root CLI commands should not reconstruct schema views from the raw schema getter"

    );

    assert!(

        lark.contains("table_header_layout("),

        "root CLI Lark helper should delegate schema/header planning to runtime"

    );

    assert!(

        !lark.contains("session.schema_view()")

            && !lark.contains("CftSchemaView::new(session.schema())"),

        "root CLI Lark helper should not own schema query construction"

    );

}



#[test]

fn root_cli_uses_session_accessors_instead_of_fields() {

    let command_modules = [

        (

            "src/commands.rs",

            std::fs::read_to_string("src/commands.rs").expect("read root CLI commands"),

        ),

        (

            "src/data_commands.rs",

            std::fs::read_to_string("src/data_commands.rs").expect("read root CLI data commands"),

        ),

        (

            "src/data_commands/lark.rs",

            std::fs::read_to_string("src/data_commands/lark.rs")

                .expect("read root CLI Lark data command helpers"),

        ),

    ];

    let forbidden = [

        "session.project.",

        "session.schema.",

        "session.model.",

        "session.diagnostics.",

        "session.sources.",

        "session.records.",

        "session.files.",

        "&session.schema",

        "&session.model",

    ];



    for (path, source) in command_modules {

        for field_access in forbidden {

            assert!(

                !source.contains(field_access),

                "{path} should use ProjectSession accessors instead of `{field_access}`"

            );

        }

    }

}



