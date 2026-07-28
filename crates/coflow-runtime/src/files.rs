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

/// Options for [`crate::ProjectQueries::file_tree_with`].
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
