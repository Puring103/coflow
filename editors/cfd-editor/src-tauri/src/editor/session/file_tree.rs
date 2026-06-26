//! Build the editor's file-tree view of a project.
//!
//! Files are surfaced when either (a) they are reported by a loader as the
//! origin of records (`in_sources`), or (b) their extension is registered
//! by some loader. Directories are surfaced implicitly as parents.
use std::collections::BTreeSet;
use std::path::Path;

use crate::editor::types::FileTreeNode;

use super::path::path_to_slash;

pub(super) fn build_file_tree(
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
        // Skip files that live under an explicitly excluded directory.
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
    roots
}

pub(super) fn build_dimension_subtree(
    root: &Path,
    group_name: impl Into<String>,
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

    Some(FileTreeNode {
        name: group_name.into(),
        path: path_to_slash(dir.strip_prefix(root).unwrap_or(dir)),
        is_dir: true,
        in_sources: true,
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    #[test]
    fn dimension_subtree_uses_real_project_paths() {
        let root =
            std::env::temp_dir().join(format!("cfd-editor-dimension-tree-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("clean temp dir");
        }
        let dim_dir = root.join("data/dimensions/language");
        std::fs::create_dir_all(&dim_dir).expect("create dimension dir");
        std::fs::write(
            dim_dir.join("Item_name.csv"),
            "id,default,zh\npotion,Potion,药水\n",
        )
        .expect("write dimension csv");

        let in_sources = BTreeSet::from(["data/dimensions/language/Item_name.csv".to_string()]);
        let ext_whitelist = BTreeSet::from(["csv".to_string()]);
        let node = build_dimension_subtree(&root, "本地化", &dim_dir, &in_sources, &ext_whitelist)
            .expect("dimension node");

        assert_eq!(node.name, "本地化");
        assert_eq!(node.path, "data/dimensions/language");
        assert_eq!(
            node.children[0].path,
            "data/dimensions/language/Item_name.csv"
        );

        std::fs::remove_dir_all(root).expect("remove temp dir");
    }
}
