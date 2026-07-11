use super::*;

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

