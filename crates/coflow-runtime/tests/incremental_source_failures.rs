#![allow(clippy::expect_used)]

use coflow_runtime::{normalize_path, DataSourceTextOverride, Project, Runtime};

fn create_project(data: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("project directory");
    std::fs::write(
        root.path().join("coflow.yaml"),
        format!(
            "schema: schema.cft\ndata: {data}\ncodegen: [{{language: csharp, dir: generated}}]\n"
        ),
    )
    .expect("config");
    std::fs::write(
        root.path().join("schema.cft"),
        "table Item { value: int; check Positive { value > 0; } }",
    )
    .expect("schema");
    root
}

#[test]
fn failed_disk_read_does_not_match_the_last_successful_text_override() {
    let root = create_project("data.cfd");
    let path = root.path().join("data.cfd");
    let original = "item: Item { value: -1 }\n";
    std::fs::write(&path, original).expect("initial data");
    let project = Project::open_schema_only(Some(root.path())).expect("project");
    let original_session = Runtime::new()
        .open_write_session(project.clone())
        .expect("initial session");
    // 非 UTF-8 文件仍存在，稳定触发读取错误而不依赖系统锁或权限配置。
    std::fs::write(&path, [0xff, 0xfe, 0xff]).expect("unreadable text");
    let failed = original_session
        .refresh_context()
        .refresh_data_sources(std::slice::from_ref(&path))
        .expect("failed source generation");
    assert!(failed
        .queries()
        .diagnostics()
        .as_set()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "CFD-READ"));
    let overrides = [DataSourceTextOverride {
        normalized_path: normalize_path(&path),
        source: original.to_owned(),
        deleted: false,
    }];
    let expected = Runtime::new()
        .open_read_only_session_with_source_overrides(project, &overrides)
        .expect("full override validation");
    let actual = failed.validation_context().validate(&overrides);
    assert_eq!(
        serde_json::to_value(&actual).expect("incremental diagnostics"),
        serde_json::to_value(expected.queries().diagnostics().as_set().flat_diagnostics())
            .expect("full diagnostics"),
    );
    assert!(!actual
        .iter()
        .any(|diagnostic| diagnostic.code == "CFD-READ"));
    assert_eq!(original_session.queries().record_count(), 1);
}

#[test]
fn source_discovery_diagnostics_keep_their_order_across_cached_refreshes() {
    let root = create_project("[before.txt, data.cfd, after.txt]");
    std::fs::write(root.path().join("before.txt"), "unsupported").expect("first source");
    std::fs::write(root.path().join("after.txt"), "unsupported").expect("last source");
    let path = root.path().join("data.cfd");
    std::fs::write(&path, "item: Item { value: -1 }\n").expect("data source");
    let project = Project::open_schema_only(Some(root.path())).expect("project");
    let mut session = Runtime::new()
        .open_write_session(project.clone())
        .expect("initial session");
    assert_eq!(session.queries().record_count(), 1);
    for source in ["item: Item { value: 2 }\n", "item: Item { value: -2 }\n"] {
        std::fs::write(&path, source).expect("edit source");
        session = session
            .refresh_context()
            .refresh_data_sources(std::slice::from_ref(&path))
            .expect("cached refresh");
        let full = Runtime::new()
            .open_read_only_session(project.clone())
            .expect("full generation");
        assert_eq!(
            session.queries().diagnostics().as_set(),
            full.queries().diagnostics().as_set(),
        );
        assert_eq!(
            session
                .queries()
                .diagnostics()
                .as_set()
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "CFD-SOURCE")
                .count(),
            2,
        );
    }
}
