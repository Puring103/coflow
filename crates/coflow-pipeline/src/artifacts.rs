use crate::{CodegenTarget, DataFormat};
use coflow_cft::CftContainer;
use coflow_codegen_csharp_json::GeneratedFile;
use coflow_codegen_csharp_json::{
    generate_csharp_json_with_key_as_enum_variants, preflight_csharp_codegen, CsharpCodegenOptions,
};
use coflow_codegen_csharp_json::{CsharpCodegenDiagnostic, CsharpKeyAsEnumVariant};
use coflow_codegen_csharp_messagepack::generate_csharp_messagepack_with_key_as_enum_variants;
use coflow_data_model::CfdDataModel;
use coflow_exporter_json::export_json_model;
use coflow_exporter_messagepack::export_messagepack_model;
use coflow_project::{OutputConfig, Project};
use serde::Serialize;
use std::collections::BTreeMap;
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
    schema: &CftContainer,
    model: &CfdDataModel,
    format: DataFormat,
    dir: &Path,
) -> Result<(), String> {
    stage_data_tables(schema, model, format, dir)?.commit()
}

pub fn stage_data_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
    format: DataFormat,
    dir: &Path,
) -> Result<StagedArtifactDir, String> {
    match format {
        DataFormat::Json => stage_json_tables(schema, model, dir),
        DataFormat::Messagepack => stage_messagepack_tables(schema, model, dir),
    }
}

pub fn stage_csharp_files(
    schema: &CftContainer,
    data_format: DataFormat,
    namespace: &str,
    dir: &Path,
    key_as_enum_variants: BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
) -> Result<StagedArtifactDir, String> {
    let options = CsharpCodegenOptions::new(namespace);
    let files = match data_format {
        DataFormat::Json => {
            generate_csharp_json_with_key_as_enum_variants(schema, &options, key_as_enum_variants)
        }
        DataFormat::Messagepack => generate_csharp_messagepack_with_key_as_enum_variants(
            schema,
            &options,
            key_as_enum_variants,
        ),
    }
    .map_err(|err| format!("failed to generate C# code: {err}"))?;
    let staged = StagedArtifactDir::create(dir)?;
    write_generated_files(staged.path(), files)?;
    Ok(staged)
}

pub fn preflight_csharp_files(
    schema: &CftContainer,
    namespace: &str,
) -> Vec<CsharpCodegenDiagnostic> {
    let options = CsharpCodegenOptions::new(namespace);
    preflight_csharp_codegen(schema, &options, &BTreeMap::new())
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
    required_format: DataFormat,
    command: &str,
) -> Result<&'a OutputConfig, String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.data; required `type: {}` and `dir` for `{command}`",
            required_format.as_config_value()
        )
    })?;
    require_output_type(output, "data", required_format.as_config_value(), command)?;
    Ok(output)
}

pub fn required_code_output<'a>(
    project: &'a Project,
    required_target: CodegenTarget,
    command: &str,
) -> Result<&'a OutputConfig, String> {
    let output = project.config.outputs.code.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.code; required `type: {}` and `dir` for `{command}`",
            required_target.as_config_value()
        )
    })?;
    require_output_type(output, "code", required_target.as_config_value(), command)?;
    Ok(output)
}

pub fn configured_data_format(project: &Project, command: &str) -> Result<DataFormat, String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `{command}`"
        )
    })?;
    DataFormat::from_config_value(&output.output_type).ok_or_else(|| {
        format!(
            "coflow.yaml outputs.data.type is `{}`; expected `json` or `messagepack`",
            output.output_type
        )
    })
}

pub fn configured_data_output<'a>(
    project: &'a Project,
    command: &str,
) -> Result<(&'a OutputConfig, DataFormat), String> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        format!(
            "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `{command}`"
        )
    })?;
    let format = DataFormat::from_config_value(&output.output_type).ok_or_else(|| {
        format!(
            "coflow.yaml outputs.data.type is `{}`; expected `json` or `messagepack`",
            output.output_type
        )
    })?;
    Ok((output, format))
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

fn stage_json_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
    dir: &Path,
) -> Result<StagedArtifactDir, String> {
    let tables = export_json_model(schema, model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    let staged = StagedArtifactDir::create(dir)?;
    for (table, value) in tables {
        let path = staged.path().join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(staged)
}

fn stage_messagepack_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
    dir: &Path,
) -> Result<StagedArtifactDir, String> {
    let tables = export_messagepack_model(schema, model)
        .map_err(|err| format!("failed to export MessagePack model: {err}"))?;
    let staged = StagedArtifactDir::create(dir)?;
    for (table, bytes) in tables {
        let path = staged.path().join(format!("{table}.msgpack"));
        fs::write(&path, bytes)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(staged)
}

fn write_generated_files(dir: &Path, files: Vec<GeneratedFile>) -> Result<(), String> {
    for file in files {
        let path = safe_artifact_path(dir, &file.relative_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
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
