use super::*;

#[test]
fn engine_public_api_does_not_expose_checker_dependency_graph() {
    let engine =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");

    assert!(
        !engine.contains("use coflow_checker::{run_checks_with_deps, DependencyGraph}"),
        "coflow-runtime should wrap checker dependency graph instead of re-exporting the checker type through ProjectSession"
    );
    assert!(
        !engine.contains("pub dependencies: coflow_checker::DependencyGraph")
            && !engine.contains("pub dependencies: DependencyGraph,"),
        "ProjectSession should not expose the checker crate dependency graph type directly"
    );
    assert!(
        !engine.contains("DependencyIndex")
            && !engine.contains("dependencies:")
            && !engine.contains("run_checks_for_dimensions_with_deps"),
        "coflow-runtime should not store or expose checker dependency indexes until an incremental-check consumer exists"
    );
}

#[test]
fn engine_runtime_indexes_do_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let indexes = std::fs::read_to_string("crates/coflow-runtime/src/indexes.rs")
        .expect("read engine indexes source");

    for expected in [
        "pub struct DiagnosticsStore",
        "pub struct SourceIndex",
        "pub struct RecordIndex",
        "pub struct FileIndex",
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
fn diagnostic_flat_views_live_with_diagnostic_types() {
    let api_diagnostics = std::fs::read_to_string("crates/coflow-api/src/diagnostics.rs")
        .expect("read coflow-api diagnostics");
    let runtime_indexes = std::fs::read_to_string("crates/coflow-runtime/src/indexes.rs")
        .expect("read runtime indexes");
    let data_read = std::fs::read_to_string("crates/coflow-runtime/src/data_read.rs")
        .expect("read runtime data read");
    let schema_inspect = std::fs::read_to_string("crates/coflow-runtime/src/schema_inspect.rs")
        .expect("read runtime schema inspect");
    let mutation_apply = std::fs::read_to_string("crates/coflow-runtime/src/mutation/apply.rs")
        .expect("read runtime mutation apply");

    assert!(
        api_diagnostics.contains("pub fn flat_diagnostics(&self) -> Vec<FlatDiagnostic>"),
        "DiagnosticSet should own flat diagnostics without logical record context"
    );
    assert!(
        runtime_indexes.contains("pub fn flat_diagnostics(&self) -> Vec<FlatDiagnostic>"),
        "DiagnosticsStore should own flat diagnostics with logical record context"
    );
    for (name, source) in [
        ("data_read.rs", data_read),
        ("schema_inspect.rs", schema_inspect),
        ("mutation/apply.rs", mutation_apply),
    ] {
        assert!(
            !source.contains("fn flat_diagnostics") && !source.contains("fn session_flat_diagnostics"),
            "{name} should call the diagnostic interface instead of owning flat conversion"
        );
    }
}

#[test]
fn engine_runtime_facade_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let runtime = std::fs::read_to_string("crates/coflow-runtime/src/runtime.rs")
        .expect("read engine runtime facade source");

    for expected in [
        "pub struct Runtime",
        "pub struct ReadOnlyProjectSession",
        "pub struct BuildProjectSession",
        "pub fn build_schema_session",
        "pub fn open_read_only_session",
        "pub fn build_project_session",
    ] {
        assert!(
            runtime.contains(expected),
            "engine runtime facade item `{expected}` should live in runtime.rs"
        );
        assert!(
            !engine.contains(expected),
            "engine runtime facade item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        engine.contains("pub use runtime::{BuildProjectSession, ReadOnlyProjectSession, Runtime};"),
        "coflow-runtime should re-export the runtime facade from lib.rs"
    );
}

#[test]
fn engine_session_api_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let session = std::fs::read_to_string("crates/coflow-runtime/src/session.rs")
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

    for forbidden in [
        "pub project: Project",
        "pub schema: CftContainer",
        "pub model: CfdDataModel",
        "pub diagnostics: DiagnosticsStore",
        "pub sources: SourceIndex",
        "pub records: RecordIndex",
        "pub files: FileIndex",
    ] {
        assert!(
            !session.contains(forbidden),
            "ProjectSession should expose `{forbidden}` through accessors, not public fields"
        );
    }

    assert!(
        session.matches("pub(crate) compiled_schema: CompiledSchema").count() == 2,
        "schema-only and full sessions should each retain one compiled schema view"
    );
    assert!(
        session.contains("pub const fn compiled_schema(&self) -> &CompiledSchema"),
        "session schema queries should borrow the retained view"
    );
}

#[test]
fn engine_schema_build_pipeline_does_not_live_in_lib_rs() {
    let engine =
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let schema_build = std::fs::read_to_string("crates/coflow-runtime/src/schema_build.rs")
        .expect("read engine schema build source");

    for expected in [
        "pub(crate) fn build_project_schema_session",
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
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let load =
        std::fs::read_to_string("crates/coflow-runtime/src/load.rs").expect("read engine load");
    let session_build = std::fs::read_to_string("crates/coflow-runtime/src/session_build.rs")
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
        "coflow-runtime lib.rs should stay below the 300-line orchestration threshold"
    );
    for expected in [
        "pub(crate) fn open_project_session",
        "pub(crate) struct SessionOpenOptions",
        "pub(crate) enum SessionIntent",
        "fn build_project_session_with_options",
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
    for forbidden in [
        "pub(crate) fn build_project_session_for_build",
        "pub(crate) fn open_project_session_read_only",
        "fn build_project_session_with_mode",
    ] {
        assert!(
            !session_build.contains(forbidden),
            "session build should use explicit SessionOpenOptions instead of `{forbidden}`"
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
        std::fs::read_to_string("crates/coflow-runtime/Cargo.toml").expect("read engine manifest");
    let data_files = std::fs::read_to_string("crates/coflow-runtime/src/data_files.rs")
        .expect("read engine data file commands");

    for forbidden in ["calamine", "umya-spreadsheet", "umya_spreadsheet"] {
        assert!(
            !manifest.contains(forbidden),
            "coflow-runtime should not depend on Excel implementation crate `{forbidden}`"
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
        std::fs::read_to_string("crates/coflow-runtime/Cargo.toml").expect("read engine manifest");
    let production_manifest = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("manifest has production dependency section");
    let data_files = std::fs::read_to_string("crates/coflow-runtime/src/data_files.rs")
        .expect("read engine data file commands");
    let dimension_regenerate =
        std::fs::read_to_string("crates/coflow-runtime/src/dimensions/regenerate.rs")
            .expect("read engine dimension regeneration");
    let session_build = std::fs::read_to_string("crates/coflow-runtime/src/session_build.rs")
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
            "coflow-runtime should use provider operations instead of depending on {forbidden}"
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
fn engine_dimension_synthesis_uses_cft_compiler_context() {
    let synthesize = std::fs::read_to_string("crates/coflow-runtime/src/dimensions/synthesize.rs")
        .expect("read dimension synthesis");

    assert!(
        synthesize.contains("schema.compiled_schema()")
            && synthesize.contains("schema.register_runtime_types(synthesized)"),
        "dimension synthesis should borrow the canonical schema and publish runtime types in one batch"
    );
    for forbidden in ["schema.all_types()", "schema.resolve_type(", ".types.get("] {
        assert!(
            !synthesize.contains(forbidden),
            "dimension synthesis should not rebuild schema traversal from `{forbidden}`"
        );
    }
}

#[test]
fn engine_schema_inspect_uses_cft_compiler_context_for_schema_traversal() {
    let schema_inspect = std::fs::read_to_string("crates/coflow-runtime/src/schema_inspect.rs")
        .expect("read schema inspect");

    assert!(
        schema_inspect.contains("session.compiled_schema()"),
        "schema inspect should borrow the session's compiled schema view"
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
fn engine_write_rules_use_cft_compiler_context_for_path_types() {
    let write_rules = std::fs::read_to_string("crates/coflow-runtime/src/write_rules.rs")
        .expect("read engine write rules");

    assert!(
        write_rules.contains("CompiledSchema::new(schema)"),
        "engine write rules should use coflow-cft CompiledSchema for schema path lookup"
    );
    assert!(
        write_rules.contains("validate_complete_value_for_schema"),
        "engine write preflight should require complete values"
    );
    assert!(
        !write_rules.contains("validate_fragment_value_for_schema"),
        "engine write preflight must not accept path fragments as complete values"
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
fn engine_data_file_headers_use_cft_compiler_context() {
    let data_files = std::fs::read_to_string("crates/coflow-runtime/src/data_files.rs")
        .expect("read engine data file commands");

    assert!(
        data_files.contains("session.compiled_schema()"),
        "data file header planning should request schema metadata through the session facade"
    );
    for expected in [
        "manager.type_for_sheet(",
        ".sheet_for_type(",
        "manager.header_options(",
    ] {
        assert!(
            data_files.contains(expected),
            "data file header planning should ask TableManager for provider option detail `{expected}`"
        );
    }
    assert!(
        data_files.contains("source.options().clone()"),
        "data file table operations should preserve configured source options without reusing configured source location"
    );
    assert!(
        data_files.contains(".table_manager_descriptors()"),
        "data file provider inference should use table manager descriptors"
    );
    assert!(
        data_files.contains("TableAddressing::Sheet"),
        "data file table layout decisions should use table manager addressing capability"
    );
    for forbidden in [
        ".resolve_type(",
        "session.schema.resolve_type",
        ".types.get(",
        ".all_fields",
        ".source_provider_descriptors()",
        "provider_id != \"cfd\"",
        "source.provider_id != \"cfd\"",
        "\"xlsx\" => Ok(\"excel\"",
        "\"cfd\" | \"csv\"",
        "\"cfd\" | \"csv\" | \"excel\"",
        "options().get(\"sheets\")",
        ".as_array()",
        "Value::as_object",
        "matching_sheet_config",
        "SourceTableConfig",
        "table_source_config",
    ] {
        assert!(
            !data_files.contains(forbidden),
            "data file commands should not bypass schema/provider facades with `{forbidden}`"
        );
    }
}

#[test]
fn engine_dimension_generation_uses_provider_source_options() {
    let dimensions = std::fs::read_to_string("crates/coflow-runtime/src/dimensions/regenerate.rs")
        .expect("read engine dimension regeneration");

    assert!(
        dimensions.contains(".source_options(&DimensionSourceOptionsRequest"),
        "dimension source options should be delegated to dimension source managers"
    );
    for forbidden in ["provider_id == \"csv\"", "\"sheets\": [{"] {
        assert!(
            !dimensions.contains(forbidden),
            "dimension generation should not hardcode provider option shape `{forbidden}`"
        );
    }
}

#[test]
fn engine_record_index_keeps_rejected_source_rows() {
    let indexes = std::fs::read_to_string("crates/coflow-runtime/src/indexes.rs")
        .expect("read engine indexes");
    let load = std::fs::read_to_string("crates/coflow-runtime/src/load.rs").expect("read load");

    assert!(
        indexes.contains("pub struct RejectedRecordRef"),
        "record index should expose rejected source record metadata"
    );
    assert!(
        load.contains("records_index.finalize_rejected_pending()"),
        "model-build diagnostics should preserve pending rejected source rows"
    );
    assert!(
        !indexes.contains("are silently dropped"),
        "rejected source records should not be documented as silently dropped"
    );
}

#[test]
fn engine_writes_use_cft_compiler_context_for_insert_schema_checks() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");

    assert!(
        writes.contains("session.compiled_schema()") || writes.contains("self.compiled_schema()"),
        "engine writes should borrow the session's compiled schema view"
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
fn engine_write_reference_planning_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");
    let refs = std::fs::read_to_string("crates/coflow-runtime/src/writes/refs.rs")
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
        "coflow-runtime writes.rs should stay below the 560-line focused-module threshold"
    );
}

#[test]
fn engine_write_target_resolution_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");
    let target = std::fs::read_to_string("crates/coflow-runtime/src/writes/target.rs")
        .expect("read engine write target helpers");

    for expected in [
        "pub(super) fn guess_new_coordinate",
        "pub(super) fn is_id_path",
        "pub(super) struct WriteTarget",
        "pub(super) fn write_target_for_path",
    ] {
        assert!(
            target.contains(expected),
            "engine write target helper `{expected}` should live in writes/target.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine write target helper `{expected}` should not live in writes.rs"
        );
    }
}

#[test]
fn engine_write_writer_dispatch_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");
    let writer_dispatch = std::fs::read_to_string("crates/coflow-runtime/src/writes/writer.rs")
        .expect("read engine writer dispatch helpers");

    for expected in [
        "pub(super) fn source_for_file",
        "pub(super) fn lookup_source_writer",
    ] {
        assert!(
            writer_dispatch.contains(expected),
            "engine writer dispatch helper `{expected}` should live in writes/writer.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine writer dispatch helper `{expected}` should not live in writes.rs"
        );
    }
    assert!(
        writes.lines().count() < 460,
        "coflow-runtime writes.rs should stay below the 460-line focused-module threshold"
    );
}

#[test]
fn engine_write_rebuild_intent_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");
    let rebuild = std::fs::read_to_string("crates/coflow-runtime/src/writes/rebuild.rs")
        .expect("read engine write rebuild helper");

    for expected in ["pub(super) fn rebuild_session_after_write", "SessionOpenOptions::build()"] {
        assert!(
            rebuild.contains(expected),
            "engine write rebuild helper `{expected}` should live in writes/rebuild.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine write rebuild helper `{expected}` should not live in writes.rs"
        );
    }
}

#[test]
fn engine_write_plan_does_not_live_in_writes_rs() {
    let writes =
        std::fs::read_to_string("crates/coflow-runtime/src/writes.rs").expect("read engine writes");
    let plan = std::fs::read_to_string("crates/coflow-runtime/src/writes/plan.rs")
        .expect("read engine write plan");

    for expected in [
        "pub(super) struct WriteFieldPlan",
        "pub(super) fn prepare_write_field",
    ] {
        assert!(
            plan.contains(expected),
            "engine write plan item `{expected}` should live in writes/plan.rs"
        );
        assert!(
            !writes.contains(expected),
            "engine write plan item `{expected}` should not live in writes.rs"
        );
    }
}

#[test]
fn engine_mutation_defaults_use_cft_compiler_context() {
    let defaults = std::fs::read_to_string("crates/coflow-runtime/src/mutation/defaults.rs")
        .expect("read mutation defaults")
        .replace("\r\n", "\n");

    assert!(
        defaults.contains("schema: &CompiledSchema"),
        "mutation default materialization should borrow the session CompiledSchema"
    );
    assert!(
        defaults.contains(".value_dependencies()"),
        "mutation default materialization should execute the compiled value dependency plan"
    );
    for forbidden in [
        "CompiledSchema::new(",
        "CftContainer",
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
fn engine_mutation_field_coercion_uses_cft_compiler_context() {
    let coercion = std::fs::read_to_string("crates/coflow-runtime/src/mutation/coercion.rs")
        .expect("read mutation coercion")
        .replace("\r\n", "\n");

    assert!(
        coercion.contains("session.compiled_schema()"),
        "mutation field coercion should borrow the session's compiled schema view"
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
fn engine_mutation_uses_cft_compiler_context_for_schema_queries() {
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation");
    let coercion = std::fs::read_to_string("crates/coflow-runtime/src/mutation/coercion.rs")
        .expect("read mutation coercion");

    assert!(
        mutation.contains("session.compiled_schema()") || coercion.contains("session.compiled_schema()"),
        "mutation schema queries should go through coflow-cft CompiledSchema"
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
        std::fs::read_to_string("crates/coflow-runtime/src/lib.rs").expect("read engine source");
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/types.rs")
        .expect("read mutation types")
        .replace("\r\n", "\n");

    assert!(
        !engine.contains("PreparedMutation"),
        "prepared mutation is an engine-internal execution detail and must not be re-exported"
    );
    assert!(
        !engine.contains("PreparedMutationOp"),
        "prepared mutation ops are an engine-internal execution detail and must not be re-exported"
    );
    assert!(
        !mutation.contains("pub enum PreparedMutationOp"),
        "external callers must not be able to construct prepared ops that bypass mutation validation"
    );
    assert!(
        !mutation.contains("pub struct PreparedMutation"),
        "external callers must not receive PreparedMutation as a public runtime type"
    );
    assert!(
        mutation.contains("pub(super) struct PreparedMutation"),
        "PreparedMutation should stay scoped to the mutation module"
    );
}

#[test]
fn engine_mutation_wire_types_do_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation module");
    let types = std::fs::read_to_string("crates/coflow-runtime/src/mutation/types.rs")
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
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation module");
    let defaults = std::fs::read_to_string("crates/coflow-runtime/src/mutation/defaults.rs")
        .expect("read mutation defaults module");

    for expected in [
        "fn default_record_for_type",
        "fn default_missing_fields_for_type",
        "struct DefaultValueMaterializer",
        "fn fields_for_type",
        "fn from_schema_default",
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
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation module");
    let coercion = std::fs::read_to_string("crates/coflow-runtime/src/mutation/coercion.rs")
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
fn engine_mutation_prepare_does_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation module");
    let prepare = std::fs::read_to_string("crates/coflow-runtime/src/mutation/prepare.rs")
        .expect("read mutation prepare module");

    for expected in [
        "fn prepare_mutation_request",
        "fn prepare_one",
        "fn prepare_insert_fields",
        "fn expected_value_for_path",
        "fn effective_write_target_for_set_field",
    ] {
        assert!(
            prepare.contains(expected),
            "mutation preparation helper `{expected}` should live in mutation/prepare.rs"
        );
        assert!(
            !mutation.contains(expected),
            "mutation preparation helper `{expected}` should not live in mutation/mod.rs"
        );
    }
    assert!(
        !prepare.contains("spread_source_path"),
        "mutation prepare should use the writes target module instead of duplicating spread source resolution"
    );
    assert!(
        mutation.lines().count() < 120,
        "coflow-runtime mutation/mod.rs should stay as a small module boundary"
    );
}

#[test]
fn engine_mutation_apply_does_not_live_in_mutation_mod_rs() {
    let mutation = std::fs::read_to_string("crates/coflow-runtime/src/mutation/mod.rs")
        .expect("read mutation module");
    let apply = std::fs::read_to_string("crates/coflow-runtime/src/mutation/apply.rs")
        .expect("read mutation apply module");

    for expected in [
        "fn apply_prepared_mutation",
        "pub fn apply_mutation",
        "fn apply_prepared_one",
        "enum MutationApplyError",
    ] {
        assert!(
            apply.contains(expected),
            "mutation apply helper `{expected}` should live in mutation/apply.rs"
        );
        assert!(
            !mutation.contains(expected),
            "mutation apply helper `{expected}` should not live in mutation/mod.rs"
        );
    }
    assert!(
        !apply.contains("fn session_flat_diagnostics") && !apply.contains("fn flat_diagnostics"),
        "mutation apply should use the diagnostic interface instead of owning flat conversion"
    );
}


