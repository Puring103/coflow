//! File-tree view for the project.
//!
//! Surfaces CFD files under the project root and groups managed dimension
//! directories under a display-named virtual folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::project::path_to_slash;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub in_sources: bool,
    #[serde(default)]
    pub in_schema: bool,
    #[serde(default)]
    pub in_data: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_source_descendant: Option<String>,
    pub children: Vec<Self>,
}

/// Internal options for building the project file tree.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileTreeOptions {
    pub(crate) dimension_groups: Vec<DimensionGroup>,
    /// In-source paths (project-relative, `/`-normalised).
    pub(crate) in_sources: BTreeSet<String>,
    pub(crate) schema_roots: BTreeSet<String>,
    pub(crate) data_roots: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DimensionGroup {
    /// Display label shown at the top of the dimension's virtual subtree
    /// (e.g. `"本地化"`).
    pub(crate) display_name: String,
    /// Absolute path of the dimension's output directory.
    pub(crate) dir: PathBuf,
}

pub fn build_file_tree(
    root: &Path,
    in_sources: &BTreeSet<String>,
    schema_roots: &BTreeSet<String>,
    data_roots: &BTreeSet<String>,
    skip_dirs: &BTreeSet<String>,
) -> Vec<FileTreeNode> {
    let mut entries: Vec<(Vec<String>, bool)> = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let rel_for_check = path
            .strip_prefix(root)
            .map(path_to_slash)
            .unwrap_or_default();
        let (in_schema, in_data) = source_membership(&rel_for_check, schema_roots, data_roots);
        let by_extension = entry.file_type().is_file() && ext == "cfd";
        if entry.file_type().is_dir() && !in_schema && !in_data {
            continue;
        }
        if entry.file_type().is_file() && !by_extension && !in_sources.contains(&rel_for_check) {
            continue;
        }
        if skip_dirs
            .iter()
            .any(|dir| rel_for_check == *dir || rel_for_check.starts_with(&format!("{dir}/")))
        {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        if !parts.is_empty() {
            entries.push((parts, entry.file_type().is_dir()));
        }
    }

    let mut roots: Vec<FileTreeNode> = Vec::new();
    for (parts, terminal_is_dir) in entries {
        insert_path(
            &mut roots,
            &parts,
            0,
            "",
            terminal_is_dir,
            in_sources,
            schema_roots,
            data_roots,
        );
    }
    sort_tree(&mut roots);
    annotate_first_source_descendant(&mut roots);
    roots
}

pub fn build_dimension_subtree(
    root: &Path,
    group_name: String,
    dir: &Path,
    in_sources: &BTreeSet<String>,
) -> Option<FileTreeNode> {
    if !dir.is_dir() {
        return None;
    }
    let mut files: Vec<Vec<String>> = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let rel_for_check = path
            .strip_prefix(root)
            .map(path_to_slash)
            .unwrap_or_default();
        let by_extension = ext == "cfd";
        if !by_extension && !in_sources.contains(&rel_for_check) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(dir) else {
            continue;
        };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        if !parts.is_empty() {
            files.push(parts);
        }
    }

    if files.is_empty() {
        return None;
    }

    let mut children = Vec::new();
    for parts in files {
        insert_dimension_path(
            &mut children,
            &parts,
            0,
            &path_to_slash(dir.strip_prefix(root).unwrap_or(dir)),
            in_sources,
        );
    }
    sort_tree(&mut children);
    annotate_first_source_descendant(&mut children);

    Some(FileTreeNode {
        name: group_name,
        path: path_to_slash(dir.strip_prefix(root).unwrap_or(dir)),
        is_dir: true,
        in_sources: true,
        in_schema: false,
        in_data: false,
        first_source_descendant: first_source_descendant(&children),
        children,
    })
}

fn insert_path(
    nodes: &mut Vec<FileTreeNode>,
    parts: &[String],
    idx: usize,
    parent_path: &str,
    terminal_is_dir: bool,
    in_sources: &BTreeSet<String>,
    schema_roots: &BTreeSet<String>,
    data_roots: &BTreeSet<String>,
) {
    if idx >= parts.len() {
        return;
    }
    let name = &parts[idx];
    let path = if parent_path.is_empty() {
        name.clone()
    } else {
        format!("{parent_path}/{name}")
    };
    let is_dir = idx + 1 < parts.len() || terminal_is_dir;

    let existing = nodes.iter_mut().find(|n| n.name == *name);
    if let Some(node) = existing {
        if is_dir {
            insert_path(&mut node.children, parts, idx + 1, &path, terminal_is_dir, in_sources, schema_roots, data_roots);
        }
        return;
    }
    let (in_schema, in_data) = source_membership(&path, schema_roots, data_roots);
    let in_src = is_dir || in_sources.contains(&path);
    let mut node = FileTreeNode {
        name: name.clone(),
        path: path.clone(),
        is_dir,
        in_sources: in_src,
        in_schema,
        in_data,
        first_source_descendant: None,
        children: Vec::new(),
    };
    if is_dir {
        insert_path(&mut node.children, parts, idx + 1, &path, terminal_is_dir, in_sources, schema_roots, data_roots);
    }
    nodes.push(node);
}

fn insert_dimension_path(
    nodes: &mut Vec<FileTreeNode>,
    parts: &[String],
    idx: usize,
    display_root: &str,
    in_sources: &BTreeSet<String>,
) {
    if idx >= parts.len() {
        return;
    }
    let name = &parts[idx];
    let rel_path = parts[..=idx].join("/");
    let path = if display_root.is_empty() {
        rel_path
    } else {
        format!("{display_root}/{rel_path}")
    };
    let is_dir = idx + 1 < parts.len();

    let existing = nodes.iter_mut().find(|n| n.name == *name);
    if let Some(node) = existing {
        if is_dir {
            insert_dimension_path(&mut node.children, parts, idx + 1, display_root, in_sources);
        }
        return;
    }
    let mut node = FileTreeNode {
        name: name.clone(),
        path: path.clone(),
        is_dir,
        in_sources: is_dir || in_sources.contains(&path),
        in_schema: false,
        in_data: false,
        first_source_descendant: None,
        children: Vec::new(),
    };
    if is_dir {
        insert_dimension_path(&mut node.children, parts, idx + 1, display_root, in_sources);
    }
    nodes.push(node);
}

fn source_membership(
    path: &str,
    schema_roots: &BTreeSet<String>,
    data_roots: &BTreeSet<String>,
) -> (bool, bool) {
    let belongs = |configured: &str| {
        path == configured
            || path.starts_with(&format!("{configured}/"))
            || configured.starts_with(&format!("{path}/"))
    };
    (
        schema_roots.iter().any(|root| belongs(root)),
        data_roots.iter().any(|root| belongs(root)),
    )
}

fn sort_tree(nodes: &mut Vec<FileTreeNode>) {
    nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    for node in nodes {
        if !node.children.is_empty() {
            sort_tree(&mut node.children);
        }
    }
}

fn annotate_first_source_descendant(nodes: &mut [FileTreeNode]) {
    for node in nodes {
        annotate_first_source_descendant(&mut node.children);
        node.first_source_descendant = if !node.is_dir && node.in_sources {
            Some(node.path.clone())
        } else {
            first_source_descendant(&node.children)
        };
    }
}

fn first_source_descendant(nodes: &[FileTreeNode]) -> Option<String> {
    nodes
        .iter()
        .find_map(|node| node.first_source_descendant.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_empty_directories_are_preserved_with_their_group() {
        let root = tempfile::tempdir().expect("temp project");
        std::fs::create_dir_all(root.path().join("schema/nested/empty")).expect("schema dirs");
        std::fs::create_dir_all(root.path().join("data/archive/empty")).expect("data dirs");
        let schema_roots = BTreeSet::from(["schema".to_string()]);
        let data_roots = BTreeSet::from(["data".to_string()]);

        let tree = build_file_tree(
            root.path(),
            &BTreeSet::new(),
            &schema_roots,
            &data_roots,
            &BTreeSet::new(),
        );

        let schema = tree.iter().find(|node| node.path == "schema").expect("schema root");
        let data = tree.iter().find(|node| node.path == "data").expect("data root");
        assert!(schema.in_schema && !schema.in_data);
        assert_eq!(schema.children[0].children[0].path, "schema/nested/empty");
        assert!(data.in_data && !data.in_schema);
        assert_eq!(data.children[0].children[0].path, "data/archive/empty");
    }
}
