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
fn inline_dimensions_do_not_create_synthetic_file_tree_groups() {
    let temp = tempfile::tempdir().expect("temp");
    fs::write(
        temp.path().join("schema.cft"),
        "table Item { @localized name: string; }",
    )
    .expect("schema");
    fs::write(
        temp.path().join("items.cfd"),
        "one: Item { name: dimension { default: \"Name\", zh: \"名称\" } }",
    )
    .expect("data");
    fs::write(
        temp.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: items.cfd\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .expect("config");
    let session = Runtime::new()
        .open_read_only_session(Project::open(Some(temp.path())).expect("project"))
        .expect("session");
    let tree = session.queries().file_tree();
    assert_eq!(count_file(&tree, "items.cfd"), 1);
    assert!(tree.iter().all(|node| node.name != "本地化"), "{tree:?}");
    assert_eq!(session.queries().dimensions()[0].variants, ["zh"]);
}
