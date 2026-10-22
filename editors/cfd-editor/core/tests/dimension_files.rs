#![allow(clippy::expect_used)]

use cfd_editor_core::editor::SessionStore;
use coflow_runtime::{CfdValue, DimensionValueCoordinate, DimensionValueState};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn coordinate(key: &str, field: &str, dimension: &str, variant: &str) -> DimensionValueCoordinate {
    DimensionValueCoordinate {
        actual_type: "Item".try_into().expect("type"),
        record_id: key.try_into().expect("key"),
        field: field.try_into().expect("field"),
        dimension: dimension.try_into().expect("dimension"),
        variant: variant.try_into().expect("variant"),
        path: vec![],
    }
}
fn value(text: &str) -> DimensionValueState {
    DimensionValueState::Value(CfdValue::String(text.into()))
}

#[test]
fn editor_groups_new_overrides_by_owner_and_dimension_and_keeps_scattered_sources() {
    for explicit in [false, true] {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "coflow-editor-dimension-files-{}-{explicit}",
            std::process::id(),
        )));
        let root = &fixture.0;
        fs::create_dir_all(root.join("data")).expect("data");
        fs::write(root.join("schema.cft"),
            "table Item { @localized name: string; @localized description: string; @dimension(\"region\") hint: string; }").expect("schema");
        fs::write(root.join("data/items.cfd"),
            "one: Item { name: \"One\", description: \"First\", hint: \"Hint\" }\ntwo: Item { name: \"Two\", description: \"Second\", hint: \"Hint\" }").expect("items");
        fs::write(
            root.join("data/other.cfd"),
            "three: Item { name: \"Three\", description: \"Third\", hint: \"Hint\" }",
        )
        .expect("other");
        fs::write(
            root.join("data/0-scattered.cfd"),
            "# keep source\nlanguage[ja] { one: Item { name: \"旧名\" } }",
        )
        .expect("overlay");
        let config = format!("{}dimensions:\n  language:\n    dir: dimensions/language\n  region:\n    dir: dimensions/region\n", if explicit {
            "schema: schema.cft\ndata: [data/items.cfd, data/other.cfd, data/0-scattered.cfd]\ncodegen: [{ language: csharp, dir: generated }]\n"
        } else { "schema: schema.cft\ndata: data/\ncodegen: [{ language: csharp, dir: generated }]\n" });
        fs::write(root.join("coflow.yaml"), &config).expect("config");
        let store = SessionStore::new().expect("store");
        let bootstrap = store.load_project(&root.join("coflow.yaml")).expect("load");
        assert_eq!(
            bootstrap.first_source_file.as_deref(),
            Some("data/items.cfd")
        );
        let session = bootstrap.session_id;
        let pending = store
            .get_dimension_file_records(
                session,
                "@dimension/language/dimensions/language/data/items.cfd",
            )
            .expect("pending file view");
        assert_eq!(pending.rows.len(), 4);
        assert!(!root.join("dimensions").exists());
        let existing = coordinate("one", "name", "language", "ja");
        store
            .write_dimension_value(session, &existing, &value("旧名"), &value("新名"), None)
            .expect("existing write");
        assert!(!root.join("dimensions/language/data/items.cfd").exists());
        let scattered = fs::read_to_string(root.join("data/0-scattered.cfd")).expect("source");
        assert!(scattered.starts_with("# keep source") && scattered.contains("新名"));

        for (target, text, file) in [
            (
                coordinate("one", "description", "language", "ja"),
                "説明",
                "dimensions/language/data/items.cfd",
            ),
            (
                coordinate("two", "name", "language", "en"),
                "Second",
                "dimensions/language/data/items.cfd",
            ),
            (
                coordinate("three", "name", "language", "ja"),
                "三",
                "dimensions/language/data/other.cfd",
            ),
            (
                coordinate("one", "hint", "region", "cn"),
                "提示",
                "dimensions/region/data/items.cfd",
            ),
        ] {
            let outcome = store
                .write_dimension_value(
                    session,
                    &target,
                    &DimensionValueState::Missing,
                    &value(text),
                    None,
                )
                .expect("new write");
            assert_eq!(outcome.affected_files, [file]);
            assert!(outcome
                .dimensions
                .iter()
                .any(|dimension| dimension.name == target.dimension.as_str()
                    && dimension.variants.contains(&target.variant.to_string())));
            fn has_dimension_file(nodes: &[coflow_runtime::FileTreeNode], file: &str) -> bool {
                nodes.iter().any(|node| {
                    node.path == file && node.dimension_only
                        || has_dimension_file(&node.children, file)
                })
            }
            assert!(has_dimension_file(&outcome.file_tree, file));
            assert_eq!(
                store
                    .get_dimension_value(session, &target)
                    .expect("read")
                    .state,
                value(text)
            );
        }
        let grouped =
            fs::read_to_string(root.join("dimensions/language/data/items.cfd")).expect("grouped");
        assert!(grouped.contains("language[ja]") && grouped.contains("language[en]"));
        assert_eq!(
            fs::read_to_string(root.join("data/0-scattered.cfd")).expect("scattered"),
            scattered
        );

        // 清空与恢复使用实际来源文件，保留撤销/重做的来源身份。
        let target = coordinate("one", "description", "language", "ja");
        store
            .write_dimension_value(
                session,
                &target,
                &value("説明"),
                &DimensionValueState::Missing,
                None,
            )
            .expect("clear");
        store
            .write_dimension_value(
                session,
                &target,
                &DimensionValueState::Missing,
                &value("説明"),
                Some("dimensions/language/data/items.cfd"),
            )
            .expect("restore");
        assert_eq!(
            fs::read_to_string(root.join("coflow.yaml")).expect("config"),
            config
        );
        let grouped_view = store
            .get_dimension_file_records(
                session,
                "@dimension/language/dimensions/language/data/items.cfd",
            )
            .expect("grouped view");
        assert_eq!(grouped_view.rows.len(), 4);
        assert!(grouped_view
            .rows
            .iter()
            .all(|row| row.owner_file_path == "data/items.cfd"));
        let scattered_view = store
            .get_dimension_file_records(session, "@dimension/language/data/0-scattered.cfd")
            .expect("scattered view");
        assert_eq!(scattered_view.rows.len(), 1);
        assert_eq!(scattered_view.rows[0].field, "name");
        assert_eq!(
            scattered_view.rows[0]
                .sources
                .get("ja")
                .and_then(|source| source.as_deref()),
            Some("data/0-scattered.cfd")
        );
        let snapshot = store.reload_session(session).expect("reload");
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        let mut nodes = snapshot.file_tree.iter().collect::<Vec<_>>();
        while let Some(node) = nodes.pop() {
            nodes.extend(&node.children);
            if !node.is_dir && node.path.ends_with(".cfd") {
                assert_eq!(
                    node.dimension_only,
                    node.path != "data/items.cfd" && node.path != "data/other.cfd",
                    "{}",
                    node.path
                );
            }
        }
    }
}
