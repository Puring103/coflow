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
