#![allow(clippy::expect_used)]

use coflow_runtime::{
    canonicalize_path, normalize_path, path_to_slash, project_path, resolve_existing_or_future_path,
};
use std::{fs, path::Path};

#[test]
fn identities_survive_file_creation_and_deletion() {
    let temp = tempfile::tempdir().expect("directory");
    let root = canonicalize_path(temp.path()).expect("root");
    let future = temp.path().join("new/sub/../file.cfd");
    let before = resolve_existing_or_future_path(&future).expect("future path");
    fs::create_dir_all(root.join("new/sub")).expect("parents");
    fs::write(&future, "").expect("file");
    assert_eq!(normalize_path(&future), before);
    fs::remove_file(&future).expect("delete");
    assert_eq!(normalize_path(&future), before);
    assert_eq!(project_path(&root, &future), "new/file.cfd");
}

#[test]
fn external_paths_keep_root_and_component_boundaries() {
    let temp = tempfile::tempdir().expect("directory");
    let root = temp.path().join("game");
    let external = temp.path().join("game-other/file.cfd");
    fs::create_dir_all(&root).expect("root");
    let displayed = project_path(&root, &external);
    assert!(Path::new(&displayed).is_absolute(), "{displayed}");
    assert_eq!(
        normalize_path(Path::new(&displayed)),
        normalize_path(&external)
    );
}

#[cfg(windows)]
#[test]
fn windows_normal_and_verbatim_paths_have_the_same_identity() {
    let temp = tempfile::tempdir().expect("directory");
    fs::write(temp.path().join("file.cfd"), "").expect("file");
    let extended = fs::canonicalize(temp.path()).expect("extended root");
    let normal = canonicalize_path(temp.path()).expect("normal root");
    assert_eq!(normalize_path(&extended), normal);
    assert_eq!(
        project_path(&extended, &normal.join("file.cfd")),
        "file.cfd"
    );
    assert_eq!(
        project_path(&normal, &extended.join("file.cfd")),
        "file.cfd"
    );
    assert_eq!(
        path_to_slash(Path::new(r"\\?\UNC\server\share\file.cfd")),
        "//?/UNC/server/share/file.cfd"
    );
}

#[cfg(unix)]
#[test]
fn unix_display_keeps_root_and_literal_backslashes() {
    assert_eq!(path_to_slash(Path::new("/tmp/a\\b.cfd")), "/tmp/a\\b.cfd");
}
