#![allow(clippy::expect_used, clippy::similar_names)]

use std::fs;
use std::path::Path;
use std::process::Command;

use coflow_runtime::{Project, ProjectDiffChange, Runtime};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_project(root: &Path, schema: &str, data: &str) {
    fs::create_dir_all(root.join("data")).expect("create data dir");
    fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("write config");
    fs::write(root.join("schema.cft"), schema).expect("write schema");
    fs::write(root.join("data/items.cfd"), data).expect("write data");
}

#[test]
fn compares_published_session_with_heads_own_project_model() {
    let repo = tempfile::tempdir().expect("temp repo");
    let project_root = repo.path().join("game");
    write_project(
        &project_root,
        "type Item { value: int; }\n",
        "Item {\n  old { value: 1, }\n  changed { value: 2, }\n}\n",
    );
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.email", "tests@coflow.local"]);
    git(repo.path(), &["config", "user.name", "Coflow Tests"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "baseline"]);

    // 当前 schema 新增默认字段，HEAD 仍必须按旧 schema 加载。
    fs::write(
        project_root.join("schema.cft"),
        "type Item { value: int; enabled: bool = true; }\n",
    )
    .expect("update schema");
    fs::write(
        project_root.join("data/items.cfd"),
        "Item {\n  changed { value: 3, }\n  added { value: 4, }\n}\n",
    )
    .expect("update data");

    let project = Project::open_schema_only(Some(&project_root)).expect("open project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("open runtime session");
    let diff = session
        .queries()
        .diff_against_head()
        .expect("diff against HEAD");

    assert_eq!(diff.head_oid.len(), 40);
    assert!(diff.semantic_available, "{:?}", diff.diagnostics);
    assert_eq!(diff.files.len(), 2);
    assert!(diff.files.iter().any(|file| {
        file.path == "data/items.cfd"
            && file.change == ProjectDiffChange::Modified
            && file
                .before
                .as_deref()
                .is_some_and(|source| source.contains("old { value: 1"))
            && file
                .after
                .as_deref()
                .is_some_and(|source| source.contains("added { value: 4"))
            && file.patch.contains("-  old { value: 1, }")
    }));

    let changes = diff
        .records
        .iter()
        .map(|record| (record.coordinate.key().to_string(), record.change))
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec![
            ("added".to_string(), ProjectDiffChange::Added),
            ("changed".to_string(), ProjectDiffChange::Modified),
            ("old".to_string(), ProjectDiffChange::Deleted),
        ]
    );
    let changed = diff
        .records
        .iter()
        .find(|record| record.coordinate.key() == "changed")
        .expect("changed record");
    assert!(changed.fields.iter().any(|field| field.path == "enabled"));
    assert!(changed.fields.iter().any(|field| field.path == "value"));
}

#[test]
fn excludes_ignored_untracked_project_sources() {
    let repo = tempfile::tempdir().expect("temp repo");
    write_project(
        repo.path(),
        "type Item { value: int; }\n",
        "Item { base { value: 1, } }\n",
    );
    fs::write(repo.path().join(".gitignore"), "data/ignored.cfd\n").expect("write ignore");
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.email", "tests@coflow.local"]);
    git(repo.path(), &["config", "user.name", "Coflow Tests"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "baseline"]);
    fs::write(
        repo.path().join("data/ignored.cfd"),
        "Item { ignored { value: 2, } }\n",
    )
    .expect("write ignored data");

    let project = Project::open_schema_only(Some(repo.path())).expect("open project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("open runtime session");
    let diff = session
        .queries()
        .diff_against_head()
        .expect("diff against HEAD");

    assert!(diff.files.is_empty());
    assert!(diff.records.is_empty());
}

#[test]
fn uses_the_published_session_instead_of_rereading_working_files() {
    let repo = tempfile::tempdir().expect("temp repo");
    write_project(
        repo.path(),
        "type Item { value: int; }\n",
        "Item { current { value: 1, } }\n",
    );
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.email", "tests@coflow.local"]);
    git(repo.path(), &["config", "user.name", "Coflow Tests"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "baseline"]);

    fs::write(
        repo.path().join("data/items.cfd"),
        "Item { current { value: 2, } }\n",
    )
    .expect("write published value");
    let project = Project::open_schema_only(Some(repo.path())).expect("open project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("open runtime session");

    // 查询开始前磁盘再次变化，配置、Schema 和数据都必须使用已经发布的 session。
    fs::write(repo.path().join("coflow.yaml"), "invalid: true\n").expect("write later config");
    fs::write(
        repo.path().join("schema.cft"),
        "type Broken { value: string; }\n",
    )
    .expect("write later schema");
    fs::write(
        repo.path().join("data/items.cfd"),
        "Item { current { value: 99, } }\n",
    )
    .expect("write later disk value");
    let diff = session
        .queries()
        .diff_against_head()
        .expect("diff against HEAD");

    let record = diff.records.first().expect("record diff");
    let after = record.fields[0].after.as_ref().expect("after value");
    assert_eq!(coflow_runtime::value_summary(after), "2");
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "data/items.cfd");
    assert!(diff.files[0]
        .patch
        .contains("+Item { current { value: 2, } }"));
    assert!(!diff.files[0].patch.contains("99"));
}

#[test]
fn keeps_source_diff_when_head_project_is_invalid() {
    let repo = tempfile::tempdir().expect("temp repo");
    write_project(
        repo.path(),
        "type Item { value: int; }\n",
        "Item { current { value: 1, } }\n",
    );
    fs::write(repo.path().join("coflow.yaml"), "not-a-project: true\n")
        .expect("write invalid HEAD config");
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.email", "tests@coflow.local"]);
    git(repo.path(), &["config", "user.name", "Coflow Tests"]);
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &["commit", "--quiet", "-m", "invalid baseline"],
    );

    fs::write(
        repo.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .expect("fix current config");
    let project = Project::open_schema_only(Some(repo.path())).expect("open current project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("open current session");
    let diff = session
        .queries()
        .diff_against_head()
        .expect("diff against HEAD");

    assert!(!diff.semantic_available);
    assert!(diff.records.is_empty());
    assert!(diff
        .files
        .iter()
        .any(|file| { file.path == "coflow.yaml" && file.patch.contains("-not-a-project: true") }));
    assert!(diff
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.endpoint == "head"));
}

#[test]
fn ignores_checkout_line_ending_differences() {
    let repo = tempfile::tempdir().expect("temp repo");
    write_project(
        repo.path(),
        "type Item { value: int; }\n",
        "Item { current { value: 1, } }\n",
    );
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.email", "tests@coflow.local"]);
    git(repo.path(), &["config", "user.name", "Coflow Tests"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "baseline"]);
    for path in ["coflow.yaml", "schema.cft", "data/items.cfd"] {
        let source = fs::read_to_string(repo.path().join(path)).expect("read source");
        fs::write(repo.path().join(path), source.replace('\n', "\r\n")).expect("write CRLF source");
    }

    let project = Project::open_schema_only(Some(repo.path())).expect("open current project");
    let session = Runtime::new()
        .open_read_only_session(project)
        .expect("open current session");
    let diff = session
        .queries()
        .diff_against_head()
        .expect("diff against HEAD");

    assert!(diff.files.is_empty());
    assert!(diff.records.is_empty());
}
