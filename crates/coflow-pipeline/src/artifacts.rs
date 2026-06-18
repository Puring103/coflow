use coflow_api::{
    ArtifactContent, ArtifactSet, CfdDataModel, CftContainer, CodegenContext, ExportContext,
    OutputSpec, ProviderRegistry,
};
use coflow_project::{OutputConfig, Project};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct StagedArtifactDir {
    target: PathBuf,
    staging: PathBuf,
    committed: bool,
}

#[derive(Debug)]
pub struct StagedArtifactFile {
    target: PathBuf,
    staging: PathBuf,
    committed: bool,
}

pub fn output_dir(
    project: &Project,
    output: &OutputConfig,
    override_dir: Option<&Path>,
) -> PathBuf {
    override_dir.map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    )
}

pub fn write_data_tables(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: &CfdDataModel,
    exporter_id: &str,
    output: &OutputConfig,
    dir: &Path,
) -> Result<(), String> {
    stage_data_tables(registry, schema, model, exporter_id, output, dir)?.commit()
}

pub fn stage_data_tables(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: &CfdDataModel,
    exporter_id: &str,
    output_config: &OutputConfig,
    dir: &Path,
) -> Result<StagedArtifactDir, String> {
    let exporter = registry
        .exporter(exporter_id)
        .ok_or_else(|| format!("no data exporter registered for `{exporter_id}`"))?;
    let output = OutputSpec {
        output_type: exporter_id.to_string(),
        dir: dir.to_path_buf(),
        options: output_options(output_config),
    };
    let artifacts = exporter
        .export(ExportContext { schema, model }, &output)
        .map_err(|diagnostics| first_diagnostic_message(&diagnostics))?;
    stage_artifact_set(dir, artifacts)
}

#[derive(Debug, Clone, Copy)]
pub struct CodegenArtifactRequest<'a> {
    pub schema: &'a CftContainer,
    pub model: Option<&'a CfdDataModel>,
    pub codegen_id: &'a str,
    pub data_format: &'a str,
    pub output_config: &'a OutputConfig,
    pub namespace: &'a str,
    pub dir: &'a Path,
    pub key_as_enum_variants: &'a Value,
}

pub fn stage_codegen_artifacts(
    registry: &ProviderRegistry,
    request: CodegenArtifactRequest<'_>,
) -> Result<StagedArtifactDir, String> {
    let codegen = registry
        .codegen(request.codegen_id)
        .ok_or_else(|| format!("no code generator registered for `{}`", request.codegen_id))?;
    let output = OutputSpec {
        output_type: request.codegen_id.to_string(),
        dir: request.dir.to_path_buf(),
        options: codegen_output_options(
            request.output_config,
            request.namespace,
            request.key_as_enum_variants,
        ),
    };
    let artifacts = codegen
        .generate(
            CodegenContext {
                schema: request.schema,
                model: request.model,
                data_format: request.data_format,
            },
            &output,
        )
        .map_err(|diagnostics| first_diagnostic_message(&diagnostics))?;
    stage_artifact_set(request.dir, artifacts)
}

pub fn preflight_codegen(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: Option<&CfdDataModel>,
    codegen_id: &str,
    data_format: &str,
    output_config: &OutputConfig,
    namespace: &str,
) -> Result<coflow_api::DiagnosticSet, String> {
    let codegen = registry
        .codegen(codegen_id)
        .ok_or_else(|| format!("no code generator registered for `{codegen_id}`"))?;
    let output = OutputSpec {
        output_type: codegen_id.to_string(),
        dir: PathBuf::new(),
        options: codegen_output_options(output_config, namespace, &Value::Null),
    };
    Ok(codegen.preflight(
        CodegenContext {
            schema,
            model,
            data_format,
        },
        &output,
    ))
}

pub fn stage_json_file<T: Serialize>(path: &Path, value: &T) -> Result<StagedArtifactFile, String> {
    StagedArtifactFile::create_json(path, value)
}

pub fn commit_staged_dir_and_file(
    dir: StagedArtifactDir,
    file: Option<StagedArtifactFile>,
) -> Result<(), String> {
    commit_staged_dirs_and_file(vec![dir], file)
}

pub fn commit_staged_dirs_and_file(
    mut dirs: Vec<StagedArtifactDir>,
    mut file: Option<StagedArtifactFile>,
) -> Result<(), String> {
    let committed_file = if let Some(file) = file.as_mut() {
        let backup = replace_file_with_staging(&file.target, &file.staging)?;
        file.committed = true;
        Some(CommittedFile {
            target: file.target.clone(),
            backup,
        })
    } else {
        None
    };

    let mut committed_dirs = Vec::new();
    for dir in &mut dirs {
        match replace_dir_with_staging(&dir.target, &dir.staging) {
            Ok(backup) => {
                dir.committed = true;
                committed_dirs.push(CommittedDir {
                    target: dir.target.clone(),
                    backup,
                });
            }
            Err(err) => {
                rollback_committed_dirs(&committed_dirs);
                if let Some(committed_file) = committed_file.as_ref() {
                    rollback_file_replace(&committed_file.target, committed_file.backup.as_deref());
                }
                return Err(err);
            }
        }
    }

    cleanup_committed_dirs(&committed_dirs)?;
    if let Some(committed_file) = committed_file {
        cleanup_committed_file(&committed_file)?;
    }
    Ok(())
}

pub fn required_data_output<'a>(
    project: &'a Project,
    exporter_id: &str,
    command: &str,
) -> Result<&'a OutputConfig, String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.data; required `type: {exporter_id}` and `dir` for `{command}`"
        )
    })?;
    require_output_type(output, "data", exporter_id, command)?;
    Ok(output)
}

pub fn required_code_output<'a>(
    project: &'a Project,
    codegen_id: &str,
    command: &str,
) -> Result<&'a OutputConfig, String> {
    let output = project.config.outputs.code.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.code; required `type: {codegen_id}` and `dir` for `{command}`"
        )
    })?;
    require_output_type(output, "code", codegen_id, command)?;
    Ok(output)
}

pub fn configured_data_format<'a>(project: &'a Project, command: &str) -> Result<&'a str, String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!("coflow.yaml missing outputs.data; required `type` and `dir` for `{command}`")
    })?;
    Ok(output.output_type.as_str())
}

pub fn configured_data_output<'a>(
    project: &'a Project,
    command: &str,
) -> Result<(&'a OutputConfig, &'a str), String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!("coflow.yaml missing outputs.data; required `type` and `dir` for `{command}`")
    })?;
    Ok((output, output.output_type.as_str()))
}

fn require_output_type(
    output: &OutputConfig,
    output_name: &str,
    required_type: &str,
    command: &str,
) -> Result<(), String> {
    if output.output_type == required_type {
        Ok(())
    } else {
        Err(format!(
            "coflow.yaml outputs.{output_name}.type is `{}`; required `{required_type}` for `{command}`",
            output.output_type
        ))
    }
}

fn output_options(output: &OutputConfig) -> Value {
    Value::Object(options_map(&output.options))
}

fn codegen_output_options(
    output: &OutputConfig,
    namespace: &str,
    key_as_enum_variants: &Value,
) -> Value {
    let mut options = options_map(&output.options);
    options.insert(
        "namespace".to_string(),
        Value::String(namespace.to_string()),
    );
    if !key_as_enum_variants.is_null() {
        options.insert(
            "key_as_enum_variants".to_string(),
            key_as_enum_variants.clone(),
        );
    }
    Value::Object(options)
}

fn options_map(options: &std::collections::BTreeMap<String, Value>) -> Map<String, Value> {
    options
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn stage_artifact_set(dir: &Path, artifacts: ArtifactSet) -> Result<StagedArtifactDir, String> {
    let staged = StagedArtifactDir::create(dir)?;
    for artifact in artifacts.files {
        let path = safe_artifact_path(staged.path(), &artifact.relative_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        match artifact.content {
            ArtifactContent::Text(contents) => fs::write(&path, contents)
                .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?,
            ArtifactContent::Bytes(bytes) => fs::write(&path, bytes)
                .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?,
            ArtifactContent::Json(value) => {
                let file = fs::File::create(&path)
                    .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
                serde_json::to_writer_pretty(file, &value)
                    .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
            }
        }
    }
    Ok(staged)
}

fn first_diagnostic_message(diagnostics: &coflow_api::DiagnosticSet) -> String {
    diagnostics.diagnostics.first().map_or_else(
        || "provider failed without diagnostics".to_string(),
        |diagnostic| diagnostic.message.clone(),
    )
}

fn safe_artifact_path(dir: &Path, relative_path: &Path) -> Result<PathBuf, String> {
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("artifact path is empty".to_string());
    }
    Ok(dir.join(relative_path))
}

impl StagedArtifactDir {
    pub fn create(target: &Path) -> Result<Self, String> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        let staging = unique_sidecar_path(target, "staging");
        if staging.exists() {
            fs::remove_dir_all(&staging)
                .map_err(|err| format!("failed to clean `{}`: {err}", staging.display()))?;
        }
        fs::create_dir(&staging)
            .map_err(|err| format!("failed to create `{}`: {err}", staging.display()))?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            committed: false,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.staging
    }

    pub fn commit(mut self) -> Result<(), String> {
        commit_staged_dir(&self.target, &self.staging)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedArtifactDir {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

impl StagedArtifactFile {
    fn create_json<T: Serialize>(target: &Path, value: &T) -> Result<Self, String> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        let staging = unique_sidecar_path(target, "staging");
        if staging.exists() {
            remove_any_path(&staging)
                .map_err(|err| format!("failed to clean `{}`: {err}", staging.display()))?;
        }
        let file = fs::File::create(&staging)
            .map_err(|err| format!("failed to create `{}`: {err}", staging.display()))?;
        serde_json::to_writer_pretty(file, value)
            .map_err(|err| format!("failed to write `{}`: {err}", staging.display()))?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            committed: false,
        })
    }
}

impl Drop for StagedArtifactFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.staging);
        }
    }
}

fn replace_file_with_staging(target: &Path, staging: &Path) -> Result<Option<PathBuf>, String> {
    if target.exists() && target.is_dir() {
        return Err(format!(
            "failed to replace `{}`: target is a directory",
            target.display()
        ));
    }
    let backup = if target.exists() {
        let backup = unique_sidecar_path(target, "backup");
        if backup.exists() {
            remove_any_path(&backup)
                .map_err(|err| format!("failed to clean `{}`: {err}", backup.display()))?;
        }
        fs::rename(target, &backup).map_err(|err| {
            format!(
                "failed to move old file `{}` to `{}`: {err}",
                target.display(),
                backup.display()
            )
        })?;
        Some(backup)
    } else {
        None
    };
    if let Err(err) = fs::rename(staging, target) {
        if let Some(backup) = backup.as_deref() {
            let _ = fs::rename(backup, target);
        }
        return Err(format!(
            "failed to move staged file `{}` to `{}`: {err}",
            staging.display(),
            target.display()
        ));
    }
    Ok(backup)
}

fn rollback_file_replace(target: &Path, backup: Option<&Path>) {
    let _ = fs::remove_file(target);
    if let Some(backup) = backup {
        let _ = fs::rename(backup, target);
    }
}

fn commit_staged_dir(target: &Path, staging: &Path) -> Result<(), String> {
    let backup = replace_dir_with_staging(target, staging)?;
    if let Some(backup) = backup {
        fs::remove_dir_all(&backup)
            .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
    }
    Ok(())
}

fn replace_dir_with_staging(target: &Path, staging: &Path) -> Result<Option<PathBuf>, String> {
    if !target.exists() {
        fs::rename(staging, target).map_err(|err| {
            format!(
                "failed to move staged artifacts `{}` to `{}`: {err}",
                staging.display(),
                target.display()
            )
        })?;
        return Ok(None);
    }
    if !target.is_dir() {
        return Err(format!(
            "failed to replace output dir `{}`: target is not a directory",
            target.display()
        ));
    }
    let backup = unique_sidecar_path(target, "backup");
    if backup.exists() {
        remove_any_path(&backup)
            .map_err(|err| format!("failed to clean `{}`: {err}", backup.display()))?;
    }
    fs::rename(target, &backup).map_err(|err| {
        format!(
            "failed to move old output dir `{}` to `{}`: {err}",
            target.display(),
            backup.display()
        )
    })?;
    match fs::rename(staging, target) {
        Ok(()) => Ok(Some(backup)),
        Err(err) => {
            let _ = fs::rename(&backup, target);
            Err(format!(
                "failed to move staged artifacts `{}` to `{}`: {err}",
                staging.display(),
                target.display()
            ))
        }
    }
}

fn rollback_dir_replace(target: &Path, backup: Option<&Path>) {
    let _ = fs::remove_dir_all(target);
    if let Some(backup) = backup {
        let _ = fs::rename(backup, target);
    }
}

#[derive(Debug)]
struct CommittedFile {
    target: PathBuf,
    backup: Option<PathBuf>,
}

#[derive(Debug)]
struct CommittedDir {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn rollback_committed_dirs(committed_dirs: &[CommittedDir]) {
    for committed_dir in committed_dirs.iter().rev() {
        rollback_dir_replace(&committed_dir.target, committed_dir.backup.as_deref());
    }
}

fn cleanup_committed_dirs(committed_dirs: &[CommittedDir]) -> Result<(), String> {
    for committed_dir in committed_dirs {
        if let Some(backup) = committed_dir.backup.as_deref() {
            fs::remove_dir_all(backup)
                .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
        }
    }
    Ok(())
}

fn cleanup_committed_file(committed_file: &CommittedFile) -> Result<(), String> {
    if let Some(backup) = committed_file.backup.as_deref() {
        fs::remove_file(backup)
            .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
    }
    Ok(())
}

fn remove_any_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn unique_sidecar_path(target: &Path, kind: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifacts");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(
        ".{name}.coflow-{kind}-{}-{suffix}",
        std::process::id()
    ))
}
