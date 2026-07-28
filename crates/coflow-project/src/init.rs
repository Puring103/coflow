use crate::diagnostics::file_error;
use crate::normalize_path;
use coflow_api::DiagnosticSet;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Default `coflow.yaml` template installed by [`init_project`].
pub const DEFAULT_PROJECT_YAML: &str = r"schema: schema/

sources: []

outputs:
  - data:
      type: json
      dir: generated/data
    code:
      type: csharp
      dir: generated/csharp
      namespace: Game.Config
    loader:
      type: csharp-json
";

/// Outcome of [`init_project`]: where the new `coflow.yaml` lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub config_path: PathBuf,
}

#[derive(Debug)]
struct InitLock {
    path: PathBuf,
    _file: File,
}

impl Drop for InitLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Default)]
struct InitTransaction {
    created_paths: Vec<PathBuf>,
    temporary_config: Option<PathBuf>,
    published_config: Option<PathBuf>,
    committed: bool,
}

impl InitTransaction {
    fn ensure_directory(&mut self, path: &Path) -> Result<(), DiagnosticSet> {
        if path.exists() {
            if path.is_dir() {
                return Ok(());
            }
            return Err(init_error(
                path,
                format!(
                    "cannot create directory `{}` because it is a file",
                    path.display()
                ),
            ));
        }
        self.created_paths.push(path.to_path_buf());
        fs::create_dir_all(path).map_err(|err| {
            init_error(
                path,
                format!("failed to create `{}`: {err}", path.display()),
            )
        })
    }

    fn write_config(&mut self, config_path: &Path) -> Result<(), DiagnosticSet> {
        let temporary = temporary_config_path(config_path);
        self.temporary_config = Some(temporary.clone());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|err| {
                init_error(
                    &temporary,
                    format!("failed to create `{}`: {err}", temporary.display()),
                )
            })?;
        file.write_all(DEFAULT_PROJECT_YAML.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|err| {
                init_error(
                    &temporary,
                    format!("failed to write `{}`: {err}", temporary.display()),
                )
            })?;
        fs::hard_link(&temporary, config_path).map_err(|err| {
            init_error(
                config_path,
                format!(
                    "failed to publish project config `{}` without overwriting: {err}",
                    config_path.display()
                ),
            )
        })?;
        self.published_config = Some(config_path.to_path_buf());
        fs::remove_file(&temporary).map_err(|err| {
            init_error(
                &temporary,
                format!(
                    "failed to remove temporary config `{}`: {err}",
                    temporary.display()
                ),
            )
        })?;
        self.temporary_config = None;
        Ok(())
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for InitTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(path) = self.temporary_config.take() {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = self.published_config.take() {
            let _ = fs::remove_file(path);
        }
        for path in self.created_paths.iter().rev() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Creates and atomically publishes a minimal Coflow project.
///
/// # Errors
///
/// Returns diagnostics when another initialization owns the target, the
/// config already exists, or any staged filesystem operation fails.
pub fn init_project(dir: impl AsRef<Path>) -> Result<InitOutcome, DiagnosticSet> {
    let dir = absolute_path(dir.as_ref())?;
    let _lock = acquire_init_lock(&dir)?;
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(init_error(
            &config_path,
            format!("`{}` already exists", config_path.display()),
        ));
    }

    let mut transaction = InitTransaction::default();
    transaction.ensure_directory(&dir)?;
    transaction.ensure_directory(&dir.join("schema"))?;
    transaction.ensure_directory(&dir.join("data"))?;
    transaction.ensure_directory(&dir.join("generated/data"))?;
    transaction.ensure_directory(&dir.join("generated/csharp"))?;
    transaction.write_config(&config_path)?;
    transaction.commit();
    Ok(InitOutcome { config_path })
}

fn absolute_path(path: &Path) -> Result<PathBuf, DiagnosticSet> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|err| init_error(path, format!("failed to resolve project directory: {err}")))
}

fn acquire_init_lock(dir: &Path) -> Result<InitLock, DiagnosticSet> {
    let lock_dir = std::env::temp_dir().join("coflow-init-locks");
    fs::create_dir_all(&lock_dir).map_err(|err| {
        init_error(
            &lock_dir,
            format!("failed to create initialization lock directory: {err}"),
        )
    })?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalize_path(dir).hash(&mut hasher);
    let path = lock_dir.join(format!("{:016x}.lock", hasher.finish()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| {
            init_error(
                dir,
                format!(
                    "project initialization for `{}` is already in progress or locked: {err}",
                    dir.display()
                ),
            )
        })?;
    Ok(InitLock { path, _file: file })
}

fn temporary_config_path(config_path: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    config_path.with_file_name(format!(
        ".coflow.yaml.{}.{sequence}.tmp",
        std::process::id()
    ))
}

fn init_error(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    file_error(path, "PROJECT-INIT-IO", "PROJECT", message)
}
