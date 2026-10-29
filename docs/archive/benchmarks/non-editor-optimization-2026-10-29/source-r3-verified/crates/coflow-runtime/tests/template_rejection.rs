#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use coflow_runtime::{Project, Runtime, SourceLocation};

#[test]
fn template_cascade_preserves_health_source_locations_old_snapshots_and_recovery() {
    let root = tempfile::tempdir().expect("project");
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("schema.cft"),
        "table Row { score: int; name: string; parent: &Row; caption: string; }",
    )
    .unwrap();
    let count = 64;
    let rows = (0..count).map(|index| format!(
        "r{index}: Row {{ score: 1, name: \"名字🙂{index}\", parent: &r{}, caption: f\"{{self.parent.name}}\" }}",
        if index == 0 { count - 1 } else { index - 1 },
    )).collect::<Vec<_>>();
    let healthy = format!(
        "{}\nkeep: Row {{score: 1, name: \"保留\", parent: &keep, caption: \"plain\"}}\n",
        rows.join("\n")
    );
    let path = root.path().join("data.cfd");
    std::fs::write(&path, &healthy).unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let original = Runtime::new()
        .open_read_only_session(project.clone())
        .unwrap();
    assert_eq!(original.queries().record_count(), count + 1);
    assert!(!original.queries().has_diagnostics());

    let invalid = healthy.replacen("score: 1", "score: \"bad\"", 1);
    std::fs::write(&path, &invalid).unwrap();
    let failed = Runtime::new()
        .open_read_only_session(project.clone())
        .unwrap();
    assert_eq!(failed.queries().record_count(), 1);
    let diagnostics = failed.queries().diagnostics().as_set();
    assert_eq!(diagnostics.diagnostics.len(), count);
    let lines = invalid.lines().collect::<Vec<_>>();
    for (index, diagnostic) in diagnostics.diagnostics.iter().enumerate() {
        let location = &diagnostic.primary.as_ref().expect("source label").location;
        let SourceLocation::FileSpan {
            start_line,
            start_character,
            end_line,
            end_character,
            ..
        } = location
        else {
            panic!("expected source span");
        };
        assert_eq!(*start_line, index, "rejection wave {index}");
        // 源值校验定位非法值，模型求值诊断沿用整条记录的 origin 范围。
        let (offset, end) = if index == 0 {
            let offset = lines[index].find("\"bad\"").unwrap();
            (offset, offset + "\"bad\"".len())
        } else {
            (0, lines[index].len())
        };
        assert_eq!(
            *start_character,
            lines[index][..offset].encode_utf16().count(),
            "UTF-16 column for rejection wave {index}"
        );
        assert_eq!(*end_line, index);
        assert_eq!(*end_character, lines[index][..end].encode_utf16().count());
    }
    let work = failed.queries().execution_stats();
    // 统计包含首个失败、依赖闭包和最终健康构建，不能退回只统计最后保留的一条。
    assert!(work.records_validated >= count * 2);
    assert!(work.records_validated <= count * 3 + 3);
    assert!(work.resolved_root_fields <= count * 12);
    assert_eq!(original.queries().record_count(), count + 1);
    assert!(!original.queries().has_diagnostics());

    std::fs::write(&path, healthy).unwrap();
    let restored = Runtime::new().open_read_only_session(project).unwrap();
    assert_eq!(restored.queries().record_count(), count + 1);
    assert!(!restored.queries().has_diagnostics());
    assert_eq!(failed.queries().record_count(), 1);
    assert_eq!(failed.queries().diagnostics().as_set(), diagnostics);
}
