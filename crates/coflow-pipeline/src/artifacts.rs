use crate::{CodegenTarget, DataFormat};
use coflow_cft::CftContainer;
use coflow_codegen_csharp_json::{
    generate_csharp_json_with_key_as_enum_variants, preflight_csharp_codegen, CsharpCodegenOptions,
};
use coflow_codegen_csharp_json::{CsharpCodegenDiagnostic, CsharpKeyAsEnumVariant};
use coflow_codegen_csharp_messagepack::generate_csharp_messagepack_with_key_as_enum_variants;
use coflow_data_model::CfdDataModel;
use coflow_exporter_json::export_json_model;
use coflow_exporter_messagepack::export_messagepack_model;
use coflow_project::{OutputConfig, Project};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Component;
use std::path::{Path, PathBuf};

const DATA_MANIFEST_FILE_NAME: &str = "coflow.data.manifest.json";
const CSHARP_MANIFEST_FILE_NAME: &str = "coflow.csharp.manifest.json";
const CSHARP_ENUM_LOCKFILE_NAME: &str = "coflow.enum.lock.json";

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
    match format {
        DataFormat::Json => write_json_tables(schema, model, dir),
        DataFormat::Messagepack => write_messagepack_tables(schema, model, dir),
    }
}

pub fn write_csharp_files(
    schema: &CftContainer,
    data_format: DataFormat,
    namespace: &str,
    dir: &Path,
    key_as_enum_variants: BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
) -> Result<(), String> {
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
    let manifest_entries = files
        .iter()
        .map(|file| artifact_manifest_entry(&file.relative_path))
        .collect::<Result<Vec<_>, _>>()?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    prepare_artifact_dir(
        dir,
        CSHARP_MANIFEST_FILE_NAME,
        &manifest_entries,
        is_generated_csharp_file,
    )?;
    for file in files {
        let path = dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    write_artifact_manifest(dir, CSHARP_MANIFEST_FILE_NAME, &manifest_entries)?;
    Ok(())
}

pub fn preflight_csharp_files(
    schema: &CftContainer,
    namespace: &str,
) -> Vec<CsharpCodegenDiagnostic> {
    let options = CsharpCodegenOptions::new(namespace);
    preflight_csharp_codegen(schema, &options, &BTreeMap::new())
}

pub fn preflight_data_artifacts(dir: &Path) -> Result<(), String> {
    preflight_artifact_dir(dir, DATA_MANIFEST_FILE_NAME, is_generated_data_file)
}

pub fn preflight_csharp_artifacts(dir: &Path) -> Result<(), String> {
    preflight_artifact_dir(dir, CSHARP_MANIFEST_FILE_NAME, is_generated_csharp_file)
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

fn write_json_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_json_model(schema, model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    let manifest_entries = tables
        .keys()
        .map(|table| artifact_manifest_entry(Path::new(&format!("{table}.json"))))
        .collect::<Result<Vec<_>, _>>()?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    prepare_artifact_dir(
        dir,
        DATA_MANIFEST_FILE_NAME,
        &manifest_entries,
        is_generated_data_file,
    )?;
    for (table, value) in tables {
        let path = dir.join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    write_artifact_manifest(dir, DATA_MANIFEST_FILE_NAME, &manifest_entries)?;
    Ok(())
}

fn write_messagepack_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_messagepack_model(schema, model)
        .map_err(|err| format!("failed to export MessagePack model: {err}"))?;
    let manifest_entries = tables
        .keys()
        .map(|table| artifact_manifest_entry(Path::new(&format!("{table}.msgpack"))))
        .collect::<Result<Vec<_>, _>>()?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    prepare_artifact_dir(
        dir,
        DATA_MANIFEST_FILE_NAME,
        &manifest_entries,
        is_generated_data_file,
    )?;
    for (table, bytes) in tables {
        let path = dir.join(format!("{table}.msgpack"));
        fs::write(&path, bytes)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    write_artifact_manifest(dir, DATA_MANIFEST_FILE_NAME, &manifest_entries)?;
    Ok(())
}

fn prepare_artifact_dir(
    dir: &Path,
    manifest_name: &str,
    current_entries: &[String],
    is_managed_file: fn(&Path) -> bool,
) -> Result<(), String> {
    let current = checked_manifest_set(current_entries, manifest_name)?;
    let previous = read_artifact_manifest(dir, manifest_name)?;
    let previous_set = checked_manifest_set(&previous, manifest_name)?;

    reject_unmanaged_artifacts(dir, &previous_set, is_managed_file)?;

    for entry in previous_set.difference(&current) {
        let path = dir.join(entry);
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|err| format!("failed to remove stale `{}`: {err}", path.display()))?;
        }
    }

    Ok(())
}

fn preflight_artifact_dir(
    dir: &Path,
    manifest_name: &str,
    is_managed_file: fn(&Path) -> bool,
) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Ok(());
    }
    let previous = read_artifact_manifest(dir, manifest_name)?;
    let previous_set = checked_manifest_set(&previous, manifest_name)?;
    reject_unmanaged_artifacts(dir, &previous_set, is_managed_file)
}

fn checked_manifest_set(
    entries: &[String],
    manifest_name: &str,
) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for entry in entries {
        validate_artifact_entry(entry, manifest_name)?;
        out.insert(entry.clone());
    }
    Ok(out)
}

fn read_artifact_manifest(dir: &Path, manifest_name: &str) -> Result<Vec<String>, String> {
    let path = dir.join(manifest_name);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read `{}`: {err}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|err| format!("failed to parse `{}`: {err}", path.display()))
}

fn write_artifact_manifest(
    dir: &Path,
    manifest_name: &str,
    entries: &[String],
) -> Result<(), String> {
    let path = dir.join(manifest_name);
    let file = fs::File::create(&path)
        .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
    serde_json::to_writer_pretty(file, entries)
        .map_err(|err| format!("failed to write `{}`: {err}", path.display()))
}

fn reject_unmanaged_artifacts(
    dir: &Path,
    previous: &BTreeSet<String>,
    is_managed_file: fn(&Path) -> bool,
) -> Result<(), String> {
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(scan_dir) = dirs.pop() {
        for entry in fs::read_dir(&scan_dir)
            .map_err(|err| format!("failed to read output dir `{}`: {err}", scan_dir.display()))?
        {
            let entry = entry
                .map_err(|err| format!("failed to read `{}` entry: {err}", scan_dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if is_artifact_sidecar(&path) {
                continue;
            }
            if !is_managed_file(&path) {
                continue;
            }
            let relative = path.strip_prefix(dir).map_err(|err| {
                format!(
                    "failed to inspect output artifact `{}` relative to `{}`: {err}",
                    path.display(),
                    dir.display()
                )
            })?;
            let relative = artifact_manifest_entry(relative)?;
            if !previous.contains(&relative) {
                return Err(format!(
                    "output dir `{}` contains unmanaged generated artifact `{}`; remove it or use a clean output directory before writing Coflow artifacts",
                    dir.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn artifact_manifest_entry(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() {
        return Err("artifact path is empty".to_string());
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    format!("artifact path `{}` is not valid UTF-8", path.display())
                })?;
                parts.push(part.replace('\\', "/"));
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(format!(
                    "artifact path `{}` must stay inside the output directory",
                    path.display()
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("artifact path is empty".to_string());
    }
    Ok(parts.join("/"))
}

fn validate_artifact_entry(entry: &str, manifest_name: &str) -> Result<(), String> {
    let normalized = artifact_manifest_entry(Path::new(entry))?;
    if normalized != entry {
        return Err(format!(
            "`{manifest_name}` contains non-normalized artifact path `{entry}`"
        ));
    }
    Ok(())
}

fn is_artifact_sidecar(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        matches!(
            name.to_str(),
            Some(DATA_MANIFEST_FILE_NAME | CSHARP_MANIFEST_FILE_NAME | CSHARP_ENUM_LOCKFILE_NAME)
        )
    })
}

fn is_generated_data_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "json" | "msgpack"))
}

fn is_generated_csharp_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "cs")
}
