use super::*;

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

        "pub(super) fn schema_files",

        "fn push_schema_path",

        "fn collect_cft_files",

        "struct SchemaDiscovery",

        "visited_directories: BTreeSet<PathBuf>",

        "visited_files: BTreeSet<PathBuf>",

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

    for forbidden in ["pub struct SchemaFile", "fn is_cft_path"] {

        assert!(

            !schema_sources.contains(forbidden),

            "project schema discovery should delegate `{forbidden}` to schema_path_policy.rs"

        );

    }

}



#[test]

fn project_schema_path_policy_owns_schema_path_rules() {

    let project =

        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");

    let policy = std::fs::read_to_string("crates/coflow-project/src/schema_path_policy.rs")

        .expect("read project schema path policy");

    let validation = std::fs::read_to_string("crates/coflow-project/src/validation.rs")

        .expect("read project validation");



    for expected in [

        "pub struct SchemaFile",

        "pub(super) struct SchemaPathPolicy",

        "pub(super) fn validate_config_path",

        "pub(super) fn is_cft_path",

        "pub(super) fn schema_file_with_identity",

        "pub(super) fn canonicalize",

        "pub(super) fn outside_declared_root_error",

    ] {

        assert!(

            policy.contains(expected),

            "schema path policy item `{expected}` should live in schema_path_policy.rs"

        );

        assert!(

            !project.contains(expected),

            "schema path policy item `{expected}` should not live in lib.rs"

        );

    }

    assert!(

        !validation.contains("fn validate_schema_path"),

        "project validation should use SchemaPathPolicy instead of owning schema path rules"

    );

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

        "pub(super) fn project_diagnostics_to_set",

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
fn project_initialization_is_an_atomic_module() {
    let project =
        std::fs::read_to_string("crates/coflow-project/src/lib.rs").expect("read project lib");
    let init =
        std::fs::read_to_string("crates/coflow-project/src/init.rs").expect("read project init");

    for expected in [
        "struct InitLock",
        "struct InitTransaction",
        "fn acquire_init_lock",
        "fs::hard_link(&temporary, config_path)",
        "pub fn init_project",
    ] {
        assert!(
            init.contains(expected),
            "atomic project initialization should own `{expected}`"
        );
        assert!(
            !project.contains(expected),
            "project lib should delegate initialization item `{expected}`"
        );
    }
}



