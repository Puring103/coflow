#![allow(clippy::expect_used, clippy::panic)]

use std::process::Command;

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
