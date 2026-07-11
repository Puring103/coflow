use super::*;

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

        manifest.contains("crates/coflow-runtime"),

        "runtime implementation should live in coflow-runtime"

    );

    assert!(

        !manifest.contains("crates/coflow-engine"),

        "coflow-engine should be removed after the runtime crate rename"

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

        "export_model_to_sink",

        "ExportEventSink",

        "pub use coflow_cft::",

        "pub use coflow_data_model::",

        "CftContainer",

        "CfdDataModel",

        "CfdValue",

    ] {

        assert!(

            !api.contains(forbidden),

            "coflow-api should not expose provider implementation algorithm `{forbidden}`"

        );

    }

}



