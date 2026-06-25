//! Build the editor's file-tree view of a project.
//!
//! Files are surfaced when either (a) they are reported by a loader as the
//! origin of records (`in_sources`), or (b) their extension is registered
//! by some loader. Directories are surfaced implicitly as parents.
use std::collections::BTreeSet;
use std::path::Path;

use crate::editor::types::FileTreeNode;

use super::path::path_to_slash;

/// Prefix used for the synthetic localization root in the file tree. The
/// front-end recognises this prefix to route reads/writes through the
/// localization-specific commands (not the generic `writeField` pipeline).
pub(super) const LOCALIZATION_ROOT: &str = "__localization__";

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
        // Skip files that live under an explicitly excluded directory
        // (e.g. the localization out_dir surfaced as a separate root).
        if skip_dirs.iter().any(|dir| {
            rel_for_check == *dir
                || rel_for_check.starts_with(&format!("{dir}/"))
        }) {
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

/// Build the synthetic "本地化" root listing every CSV under `dir`. Returns
/// `None` when the directory is missing or contains no CSV files — the
/// caller treats `None` as "skip the root entirely" so we don't show an
/// empty top-level folder when localization isn't actually used.
pub(super) fn build_localization_subtree(dir: &Path) -> Option<FileTreeNode> {
    if !dir.is_dir() {
        return None;
    }
    let mut children: Vec<FileTreeNode> = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let is_csv = p
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"));
        if !is_csv {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        children.push(FileTreeNode {
            name: name.clone(),
            path: format!("{LOCALIZATION_ROOT}/{name}"),
            is_dir: false,
            in_sources: true,
            children: Vec::new(),
        });
    }
    if children.is_empty() {
        return None;
    }
    children.sort_by(|a, b| a.name.cmp(&b.name));
    Some(FileTreeNode {
        name: "本地化".to_string(),
        path: LOCALIZATION_ROOT.to_string(),
        is_dir: true,
        in_sources: true,
        children,
    })
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
