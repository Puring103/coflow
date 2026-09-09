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
fn dimension_directory_identity_matches_tree_and_is_not_duplicated() {
    for case in [
        "relative",
        "parent",
        "absolute",
        "external",
        "external_absolute",
    ] {
        let temp = tempfile::tempdir().expect("temp");
        let project_dir = temp.path().join("project");
        fs::create_dir_all(&project_dir).expect("project directory");
        let external = case.starts_with("external");
        let data_dir = if external {
            temp.path().join("shared")
        } else {
            project_dir.join("data")
        };
        let dimension_dir = data_dir.join("language");
        fs::create_dir_all(&dimension_dir).expect("dimension directory");
        fs::write(
            project_dir.join("schema.cft"),
            "type Item { @localized name: string; }",
        )
        .expect("schema");
        fs::write(data_dir.join("items.cfd"), "one: Item { name: \"Name\" }").expect("data");
        fs::write(
            dimension_dir.join("Item_name.cfd"),
            "one: __coflow_language_Item_name { zh: \"Translation\" }",
        )
        .expect("dimension");
        let out_dir = match case {
            "relative" => "./data/language/".to_string(),
            "parent" => "data/../data/language".to_string(),
            "absolute" | "external_absolute" => fs::canonicalize(&dimension_dir)
                .expect("canonical directory")
                .to_string_lossy()
                .into_owned(),
            _ => "../shared/language".to_string(),
        };
        let config = serde_json::json!({
            "schema": "schema.cft", "data": if external { "../shared" } else { "data" },
            "dimensions": { "language": { "variants": ["zh"], "out_dir": out_dir } },
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
        let tree = session.queries().file_tree();
        let dimension = tree
            .iter()
            .find(|node| node.name == "本地化")
            .expect("dimension root");
        assert_eq!(
            dimensions[0].out_dir.as_deref(),
            Some(dimension.path.as_str()),
            "{case}"
        );
        assert_eq!(count_file(&tree, "Item_name.cfd"), 1, "{case}: {tree:?}");
        assert_eq!(count_file(&tree, "items.cfd"), 1, "{case}: base data");
    }
}
