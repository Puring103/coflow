#![allow(clippy::expect_used, clippy::panic, clippy::panic_in_result_fn)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::{Table, Value};

#[derive(Debug)]
struct WorkspaceGraph {
    members: BTreeMap<String, MemberManifest>,
}

#[derive(Debug)]
struct MemberManifest {
    path: PathBuf,
    manifest: Table,
    dependencies: BTreeSet<String>,
}

impl WorkspaceGraph {
    fn load() -> Self {
        let root = read_manifest(Path::new("Cargo.toml"));
        let member_paths = std::iter::once(PathBuf::from("."))
            .chain(
                root["workspace"]["members"]
                    .as_array()
                    .expect("workspace members")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from),
            )
            .collect::<Vec<_>>();
        let mut members = BTreeMap::new();
        for path in member_paths {
            let manifest = read_manifest(&path.join("Cargo.toml"));
            let name = manifest["package"]["name"]
                .as_str()
                .expect("package name")
                .to_string();
            let dependencies = manifest
                .get("dependencies")
                .and_then(Value::as_table)
                .map(|dependencies| dependencies.keys().cloned().collect())
                .unwrap_or_default();
            let previous = members.insert(
                name.clone(),
                MemberManifest {
                    path,
                    manifest,
                    dependencies,
                },
            );
            assert!(previous.is_none(), "duplicate workspace package `{name}`");
        }
        Self { members }
    }

    fn member(&self, name: &str) -> &MemberManifest {
        self.members
            .get(name)
            .unwrap_or_else(|| panic!("workspace package `{name}` is missing"))
    }

    fn assert_depends_on(&self, package: &str, dependency: &str) {
        assert!(
            self.member(package).dependencies.contains(dependency),
            "`{package}` must depend on `{dependency}`"
        );
    }

    fn assert_does_not_depend_on(&self, package: &str, forbidden: &[&str]) {
        let dependencies = &self.member(package).dependencies;
        let offenders = forbidden
            .iter()
            .copied()
            .filter(|dependency| dependencies.contains(*dependency))
            .collect::<Vec<_>>();
        assert!(
            offenders.is_empty(),
            "`{package}` has forbidden architecture dependencies: {offenders:?}"
        );
    }
}

#[test]
fn workspace_members_inherit_workspace_lints() {
    let graph = WorkspaceGraph::load();
    let missing = graph
        .members
        .iter()
        .filter_map(|(name, member)| {
            let inherited = member
                .manifest
                .get("lints")
                .and_then(Value::as_table)
                .and_then(|lints| lints.get("workspace"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            (!inherited).then_some((name, &member.path))
        })
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "workspace members must inherit workspace lints: {missing:?}"
    );
}

#[test]
fn dependency_graph_enforces_internal_crate_boundaries() {
    let graph = WorkspaceGraph::load();
    let concrete_providers = [
        "coflow-loader-cfd",
        "coflow-loader-csv",
        "coflow-loader-excel",
        "coflow-loader-lark",
        "coflow-exporter-json",
        "coflow-exporter-messagepack",
        "coflow-codegen-csharp",
    ];
    graph.assert_does_not_depend_on("coflow-api", &concrete_providers);
    graph.assert_does_not_depend_on("coflow-runtime", &concrete_providers);
    graph.assert_does_not_depend_on(
        "coflow-checker",
        &["coflow-project", "coflow-runtime", "coflow-api"],
    );
    graph.assert_does_not_depend_on("cfd-editor", &["coflow-checker"]);

    graph.assert_depends_on("coflow", "coflow-runtime");
    graph.assert_depends_on("cfd-editor", "coflow-runtime");
    graph.assert_depends_on("coflow-builtins", "coflow-api");
}

#[test]
fn shared_algorithms_have_dedicated_dependency_owners() {
    let graph = WorkspaceGraph::load();
    for provider in [
        "coflow-loader-csv",
        "coflow-loader-excel",
        "coflow-loader-lark",
    ] {
        graph.assert_depends_on(provider, "coflow-loader-table-core");
    }
    for exporter in ["coflow-exporter-json", "coflow-exporter-messagepack"] {
        graph.assert_depends_on(exporter, "coflow-exporter-core");
    }
    graph.assert_does_not_depend_on(
        "coflow-api",
        &["coflow-loader-table-core", "coflow-exporter-core"],
    );

    for required in ["coflow-loader-table-core", "coflow-exporter-core"] {
        assert!(
            graph.members.contains_key(required),
            "shared algorithm package `{required}` is missing"
        );
    }
    for removed in [
        "coflow-cell-value",
        "coflow-engine",
        "coflow-pipeline",
        "coflow-editor-core",
    ] {
        assert!(
            !graph.members.contains_key(removed),
            "removed shallow package `{removed}` must not return"
        );
    }
}

#[test]
fn tracked_files_do_not_include_generated_outputs() {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .output()
        .expect("run git ls-files");
    if !output.status.success() {
        return;
    }
    let stdout = String::from_utf8(output.stdout).expect("git output is utf8");
    let offenders = stdout
        .split_terminator('\0')
        .filter(|path| {
            Path::new(path)
                .components()
                .any(|component| component.as_os_str() == "generated")
                && Path::new(path).exists()
        })
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "generated outputs should not be tracked: {offenders:#?}"
    );
}

fn read_manifest(path: &Path) -> Table {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("read manifest {}: {error}", path.display()));
    let source = String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("manifest {} is not UTF-8: {error}", path.display()));
    source
        .parse::<Table>()
        .unwrap_or_else(|error| panic!("parse manifest {}: {error}", path.display()))
}
