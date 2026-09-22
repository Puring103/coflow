use std::path::Path;
use std::process::Command;

#[test]
fn engine_dependencies_stay_inside_engine() {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("engine directory");
    // 使用 Cargo 解析后的依赖路径，覆盖可选依赖、平台依赖及未来新增的 Engine crate。
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(&engine)
        .output()
        .expect("Cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("metadata JSON");
    let mut checked = 0;
    for package in metadata["packages"].as_array().expect("packages") {
        let manifest = Path::new(package["manifest_path"].as_str().expect("manifest"))
            .canonicalize()
            .expect("manifest path");
        if !manifest.starts_with(&engine) {
            continue;
        }
        checked += 1;
        for dependency in package["dependencies"].as_array().expect("dependencies") {
            if let Some(path) = dependency["path"].as_str() {
                assert!(
                    Path::new(path)
                        .canonicalize()
                        .expect("dependency path")
                        .starts_with(&engine),
                    "Engine package {} depends on a path outside Engine: {path}",
                    package["name"],
                );
            }
        }
    }
    assert!(checked >= 4, "expected the Engine workspace packages");
}
