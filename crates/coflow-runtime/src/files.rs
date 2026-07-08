//! File-tree view for the project.
//!
//! Surfaces directories and files under the project root that either back a
//! loaded record (`in_sources`) or carry an extension registered by the
//! configured providers. Dimension output directories can be grouped under a
//! display-named virtual folder via [`FileTreeOptions::dimension_groups`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use coflow_project::path_to_slash;

const REMOTE_SOURCE_GROUP_NAME: &str = "Remote";
const REMOTE_SOURCE_GROUP_PATH: &str = "__remote_sources__";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_source_descendant: Option<String>,
    pub children: Vec<Self>,
}

/// Options for [`super::ProjectSession::file_tree_with`].
///
/// Defaults mirror what the editor needs: walk every loader-registered
/// extension and pull dimension output directories into a sibling virtual
/// folder at the top of the tree.
#[derive(Debug, Clone, Default)]
pub struct FileTreeOptions {
    pub extra_extensions: Vec<String>,
    pub dimension_groups: Vec<DimensionGroup>,
    /// In-source paths reported by loaders (project-relative, `/`-normalised).
    pub in_sources: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct DimensionGroup {
    /// Display label shown at the top of the dimension's virtual subtree
    /// (e.g. `"本地化"`).
    pub display_name: String,
    /// Absolute path of the dimension's output directory.
    pub dir: PathBuf,
}

pub fn build_file_tree(
    root: &Path,
    in_sources: &BTreeSet<String>,
    ext_whitelist: &BTreeSet<String>,
    skip_dirs: &BTreeSet<String>,
) -> Vec<FileTreeNode> {
    let mut files: Vec<Vec<String>> = Vec::new();
    for entry in walkdir::WalkDir::new(root)
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
        let by_extension = !ext.is_empty() && ext_whitelist.contains(ext);
        if !by_extension && !in_sources.contains(&rel_for_check) {
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
            files.push(parts);
        }
    }

    let mut roots: Vec<FileTreeNode> = Vec::new();
    for parts in files {
        insert_path(&mut roots, &parts, 0, "", in_sources);
    }
    for source in in_sources {
        insert_virtual_source(&mut roots, source);
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
    ext_whitelist: &BTreeSet<String>,
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
        let by_extension = !ext.is_empty() && ext_whitelist.contains(ext);
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
        first_source_descendant: first_source_descendant(&children),
        children,
    })
}

fn insert_path(
    nodes: &mut Vec<FileTreeNode>,
    parts: &[String],
    idx: usize,
    parent_path: &str,
    in_sources: &BTreeSet<String>,
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
    let is_dir = idx + 1 < parts.len();

    let existing = nodes.iter_mut().find(|n| n.name == *name);
    if let Some(node) = existing {
        if is_dir {
            insert_path(&mut node.children, parts, idx + 1, &path, in_sources);
        }
        return;
    }
    let in_src = if is_dir {
        true
    } else {
        in_sources.contains(&path)
    };
    let mut node = FileTreeNode {
        name: name.clone(),
        path: path.clone(),
        is_dir,
        in_sources: in_src,
        first_source_descendant: None,
        children: Vec::new(),
    };
    if is_dir {
        insert_path(&mut node.children, parts, idx + 1, &path, in_sources);
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
        first_source_descendant: None,
        children: Vec::new(),
    };
    if is_dir {
        insert_dimension_path(&mut node.children, parts, idx + 1, display_root, in_sources);
    }
    nodes.push(node);
}

fn insert_virtual_source(nodes: &mut Vec<FileTreeNode>, source: &str) {
    if nodes.iter().any(|node| contains_path(node, source)) {
        return;
    }
    if is_local_project_path(source) {
        return;
    }
    let group_index = nodes
        .iter()
        .position(|node| node.is_dir && node.path == REMOTE_SOURCE_GROUP_PATH)
        .unwrap_or_else(|| {
            nodes.push(FileTreeNode {
                name: REMOTE_SOURCE_GROUP_NAME.to_string(),
                path: REMOTE_SOURCE_GROUP_PATH.to_string(),
                is_dir: true,
                in_sources: true,
                first_source_descendant: None,
                children: Vec::new(),
            });
            nodes.len() - 1
        });
    nodes[group_index].children.push(FileTreeNode {
        name: virtual_source_name(source),
        path: source.to_string(),
        is_dir: false,
        in_sources: true,
        first_source_descendant: None,
        children: Vec::new(),
    });
}

fn contains_path(node: &FileTreeNode, path: &str) -> bool {
    node.path == path || node.children.iter().any(|child| contains_path(child, path))
}

fn is_local_project_path(source: &str) -> bool {
    !source.contains("://")
}

fn virtual_source_name(source: &str) -> String {
    source
        .split('?')
        .next()
        .unwrap_or(source)
        .split(['/', '\\'])
        .rev()
        .find(|part| !part.is_empty())
        .unwrap_or(source)
        .to_string()
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
    fn file_tree_groups_remote_sources_as_virtual_files() {
        let mut sources = BTreeSet::new();
        let source =
            "https://fand3tbr90g.feishu.cn/wiki/F2d0wfnQyizLTAkvuuyciMNqnUe?fromScene=spaceOverview";
        sources.insert(source.to_string());

        let tree = build_file_tree(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &sources,
            &BTreeSet::new(),
            &BTreeSet::new(),
        );

        let remote = tree
            .iter()
            .find(|node| node.path == REMOTE_SOURCE_GROUP_PATH);
        assert!(
            remote.is_some(),
            "remote source group is present in the file tree"
        );
        let Some(remote) = remote else {
            return;
        };
        assert_eq!(remote.name, REMOTE_SOURCE_GROUP_NAME);
        assert!(remote.is_dir);
        assert!(remote.in_sources);
        assert_eq!(remote.children.len(), 1);

        let node = &remote.children[0];
        assert_eq!(node.path, source);
        assert_eq!(node.name, "F2d0wfnQyizLTAkvuuyciMNqnUe");
        assert!(!node.is_dir);
        assert!(node.in_sources);
    }
}
