use crate::diagnostics::file_error;
use crate::paths::resolve_project_relative;
use crate::path_to_slash;
use coflow_api::DiagnosticSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SchemaFile {
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub module_id: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SchemaPathPolicy<'a> {
    root_dir: &'a Path,
}

impl<'a> SchemaPathPolicy<'a> {
    pub(super) const fn new(root_dir: &'a Path) -> Self {
        Self { root_dir }
    }

    pub(super) fn resolve(&self, path: &Path) -> PathBuf {
        resolve_project_relative(self.root_dir, path)
    }

    pub(super) fn validate_config_path(&self, path: &Path, label: &str) -> Result<(), String> {
        if path.as_os_str().is_empty() {
            return Err(format!("{label} path is empty"));
        }
        let resolved = self.resolve(path);
        if !resolved.exists() {
            return Err(format!("{label} path `{}` does not exist", path.display()));
        }
        if resolved.is_file() && !Self::is_cft_path(&resolved) {
            return Err(format!(
                "schema file `{}` has unsupported extension",
                path_to_slash(path)
            ));
        }
        Ok(())
    }

    pub(super) fn is_cft_path(path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()) == Some("cft")
    }

    pub(super) fn unsupported_file_error(&self, path: &Path) -> DiagnosticSet {
        file_error(
            path,
            "PROJECT-SCHEMA-PATH",
            "PROJECT",
            format!(
                "schema file `{}` has unsupported extension",
                self.display_path(path)
            ),
        )
    }

    pub(super) fn missing_path_error(&self, path: &Path) -> DiagnosticSet {
        file_error(
            path,
            "PROJECT-SCHEMA-PATH",
            "PROJECT",
            format!("schema path `{}` does not exist", path.display()),
        )
    }

    pub(super) fn read_dir_error(&self, dir: &Path, err: impl std::fmt::Display) -> DiagnosticSet {
        file_error(
            dir,
            "PROJECT-SCHEMA-READ",
            "PROJECT",
            format!("failed to read schema directory `{}`: {err}", dir.display()),
        )
    }

    pub(super) fn schema_file(&self, path: PathBuf) -> Result<SchemaFile, DiagnosticSet> {
        let canonical_path = fs::canonicalize(&path).map_err(|err| {
            file_error(
                &path,
                "PROJECT-SCHEMA-PATH",
                "PROJECT",
                format!("failed to resolve schema `{}`: {err}", path.display()),
            )
        })?;
        let module_path = canonical_path
            .strip_prefix(self.root_dir)
            .unwrap_or(canonical_path.as_path());
        let module_id = path_to_slash(module_path);
        Ok(SchemaFile {
            path,
            canonical_path,
            module_id,
        })
    }

    fn display_path(&self, path: &Path) -> String {
        path_to_slash(path.strip_prefix(self.root_dir).unwrap_or(path))
    }
}
