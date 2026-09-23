#![allow(clippy::expect_used)]

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn project() -> TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; }\n",
    )
    .expect("schema");
    fs::write(
        dir.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: []\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("config");
    dir
}

fn run_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_coflow"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("CLI output")
}

#[test]
fn schema_check_failure_does_not_write_candidate() {
    let dir = project();
    let before = fs::read_to_string(dir.path().join("schema.cft")).expect("schema");
    let output = run_with_stdin(
        &[
            "schema",
            "write-file",
            dir.path().to_str().expect("path"),
            "--file",
            "schema.cft",
            "--check",
            "--json",
        ],
        "table Item {",
    );
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["written"], false);
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["check_ok"], false);
    assert_eq!(report["changed"], true);
    assert!(!report["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("schema.cft")).expect("schema"),
        before
    );
}

#[test]
fn schema_valid_candidate_writes_unless_dry_run() {
    let dir = project();
    let before = fs::read_to_string(dir.path().join("schema.cft")).expect("schema");
    let candidate = "table Item { name: string; count: int; }\n";
    let path = dir.path().to_str().expect("path");
    let args = [
        "schema",
        "write-file",
        path,
        "--file",
        "schema.cft",
        "--check",
        "--json",
    ];
    let dry_run = run_with_stdin(&[&args[..], &["--dry-run"]].concat(), candidate);
    assert!(
        dry_run.status.success(),
        "{}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let report: Value = serde_json::from_slice(&dry_run.stdout).expect("JSON report");
    assert_eq!(report["written"], false);
    assert_eq!(report["check_ok"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("schema.cft")).expect("schema"),
        before
    );

    let write = run_with_stdin(&args, candidate);
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    let report: Value = serde_json::from_slice(&write.stdout).expect("JSON report");
    assert_eq!(report["written"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("schema.cft")).expect("schema"),
        candidate
    );
}

#[test]
fn json_commands_keep_json_on_project_open_errors() {
    let dir = tempfile::tempdir().expect("temp dir");
    for args in [
        vec!["check", dir.path().to_str().expect("path"), "--json"],
        vec!["diff", dir.path().to_str().expect("path"), "--json"],
        vec![
            "schema",
            "inspect",
            dir.path().to_str().expect("path"),
            "--json",
        ],
    ] {
        let output = run_with_stdin(&args, "");
        assert!(!output.status.success());
        let result: Value = serde_json::from_slice(&output.stdout).expect("JSON diagnostics");
        assert!(!result["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty());
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn diff_with_unavailable_semantics_returns_nonzero_but_preserves_source_diff() {
    let dir = project();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    fs::write(dir.path().join("schema.cft"), "table Item {").expect("invalid HEAD schema");
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "baseline",
    ]);
    fs::write(
        dir.path().join("schema.cft"),
        "table Item { name: string; }\n",
    )
    .expect("fixed current schema");

    let path = dir.path().to_str().expect("path");
    let json = run_with_stdin(&["diff", path, "--json"], "");
    assert!(!json.status.success());
    let report: Value = serde_json::from_slice(&json.stdout).expect("JSON diff");
    assert_eq!(report["semantic_available"], false);
    assert!(!report["files"].as_array().expect("files").is_empty());
    assert!(!report["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .is_empty());

    let human = run_with_stdin(&["diff", path], "");
    assert!(!human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("Semantic diff unavailable"));
}

#[test]
fn build_command_is_no_longer_exposed() {
    let output = run_with_stdin(&["build"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}
