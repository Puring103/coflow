use super::*;

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

fn editor_backend_schema_queries_use_runtime_facade() {

    let convert = std::fs::read_to_string(

        "editors/cfd-editor/src-tauri/src/editor/convert.rs",

    )

    .expect("read editor convert");

    let session = std::fs::read_to_string(

        "editors/cfd-editor/src-tauri/src/editor/session/mod.rs",

    )

    .expect("read editor session");

    let runtime_session = std::fs::read_to_string("crates/coflow-runtime/src/session.rs")

        .expect("read runtime session");



    assert!(

        runtime_session.contains("pub fn enum_variants(&self, enum_name: &str) -> Vec<String>"),

        "runtime session should expose enum variant queries for hosts"

    );

    assert!(

        convert.contains("schema: queries.compiled_schema()")

            && convert.contains("enum_type_name(ty, &ctx.schema)")

            && convert.contains("schema.is_schema_enum(name)"),

        "editor convert should use CompiledSchema supplied by the runtime session"

    );

    assert!(

        session.contains("session.queries().enum_variants(enum_name)"),

        "editor session should query enum variants through runtime session"

    );

    for forbidden in [

        "CompiledSchema::new(session.schema())",

        "ctx.queries.schema()",

        "coflow_cft::CftContainer",

        ".schema().resolve_enum(",

    ] {

        assert!(

            !convert.contains(forbidden) && !session.contains(forbidden),

            "editor backend should not bypass runtime schema facades with `{forbidden}`"

        );

    }

}



#[test]

fn hosts_cannot_reach_the_owning_runtime_session() {

    for path in [

        "src/commands.rs",

        "src/data_commands.rs",

        "editors/cfd-editor/src-tauri/src/editor/convert.rs",

        "editors/cfd-editor/src-tauri/src/editor/session/mod.rs",

        "editors/cfd-editor/src-tauri/src/editor/session/build.rs",

    ] {

        let source = std::fs::read_to_string(path).expect("read runtime host");

        for forbidden in [

            "use coflow_runtime::ProjectSession",

            " ProjectSession,",

            " ProjectSession}",

            ".into_session()",

            ".as_session()",

        ] {

            assert!(

                !source.contains(forbidden),

                "{path} must use capability sessions instead of `{forbidden}`"

            );

        }

    }

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

        root_manifest.contains("coflow-loader-table-core ="),

        "root CLI should reuse table-core typed table option facade instead of hand-parsing provider options"

    );

    assert!(

        !root_manifest.contains("coflow-engine ="),

        "root CLI should not depend on the removed coflow-engine crate"

    );

    assert!(

        editor_manifest.contains("coflow-runtime ="),

        "editor backend should depend on coflow-runtime"

    );

    assert!(

        !editor_manifest.contains("coflow-engine ="),

        "editor backend should not depend on the removed coflow-engine crate"

    );

    assert!(

        !runtime_manifest.contains("coflow-engine ="),

        "coflow-runtime should be the implementation crate, not delegate to coflow-engine"

    );

    assert!(

        !runtime_lib.contains("pub use coflow_runtime::*;"),

        "coflow-runtime should not be a self-reexport facade"

    );

    assert!(

        !std::path::Path::new("crates/coflow-engine/Cargo.toml").exists(),

        "coflow-engine crate should not exist after the runtime rename"

    );

}



