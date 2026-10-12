#![allow(clippy::expect_used)]

use coflow_runtime::{FileTreeNode, Project, Runtime};
use std::fs;

fn count_file(nodes: &[FileTreeNode], name: &str) -> usize {
    nodes
        .iter()
        .map(|node| usize::from(node.name == name) + count_file(&node.children, name))
        .sum()
}

#[test]
fn selected_dimension_sources_keep_physical_tree_and_header_metadata() {
    for external in [false, true] {
        let temp = tempfile::tempdir().expect("temp");
        let project_dir = temp.path().join("project");
        let data_dir = if external {
            temp.path().join("shared")
        } else {
            project_dir.join("data")
        };
        fs::create_dir_all(&project_dir).expect("project directory");
        fs::create_dir_all(data_dir.join("translations")).expect("data directory");
        fs::write(
            project_dir.join("schema.cft"),
            "type Item { @localized name: string; }",
        )
        .expect("schema");
        fs::write(data_dir.join("items.cfd"), "one: Item { name: \"Name\" }").expect("base");
        fs::write(
            data_dir.join("translations/ja.cfd"),
            "language[ja] { one: Item { name: \"翻訳\" } }\nlanguage[fr] {}",
        )
        .expect("dimension");
        let config = serde_json::json!({
            "schema": "schema.cft", "data": if external { "../shared" } else { "data" },
            "dimensions": { "language": { "display_name": "语言" } },
            "codegen": [{ "language": "csharp", "dir": "generated" }],
        });
        fs::write(
            project_dir.join("coflow.yaml"),
            serde_yaml::to_string(&config).expect("yaml"),
        )
        .expect("config");
        let session = Runtime::new()
            .open_read_only_session(Project::open(Some(&project_dir)).expect("project"))
            .expect("session");
        let dimensions = session.queries().dimensions();
        assert_eq!(dimensions.len(), 1);
        assert_eq!(dimensions[0].name, "language");
        assert_eq!(dimensions[0].display_name, "语言");
        assert_eq!(dimensions[0].variants, ["fr", "ja"]);
        let tree = session.queries().file_tree();
        assert_eq!(count_file(&tree, "ja.cfd"), 1);
        assert_eq!(count_file(&tree, "items.cfd"), 1);
        assert_eq!(count_file(&tree, "语言"), 0);
    }
}
