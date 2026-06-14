use crate::{CodegenTarget, DataFormat};
use coflow_cft::CftContainer;
use coflow_codegen_csharp_json::{
    generate_csharp_json_with_key_as_enum_variants, preflight_csharp_codegen, CsharpCodegenOptions,
};
use coflow_codegen_csharp_json::{CsharpCodegenDiagnostic, CsharpKeyAsEnumVariant};
use coflow_codegen_csharp_messagepack::generate_csharp_messagepack_with_key_as_enum_variants;
use coflow_exporter_json::export_json_model;
use coflow_exporter_messagepack::export_messagepack_model;
use coflow_loader_excel::ExcelLoadOutput;
use coflow_project::{OutputConfig, Project};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    load_output: &ExcelLoadOutput,
    format: DataFormat,
    dir: &Path,
) -> Result<(), String> {
    match format {
        DataFormat::Json => write_json_tables(schema, load_output, dir),
        DataFormat::Messagepack => write_messagepack_tables(schema, load_output, dir),
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
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    clean_generated_csharp_files(dir)?;
    for file in files {
        let path = dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}

pub fn preflight_csharp_files(
    schema: &CftContainer,
    namespace: &str,
) -> Vec<CsharpCodegenDiagnostic> {
    let options = CsharpCodegenOptions::new(namespace);
    preflight_csharp_codegen(schema, &options, BTreeMap::new())
}

fn clean_generated_csharp_files(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read output dir `{}`: {err}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("failed to read `{}` entry: {err}", dir.display()))?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "cs") {
            fs::remove_file(&path)
                .map_err(|err| format!("failed to remove stale `{}`: {err}", path.display()))?;
        }
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

fn write_json_tables(
    schema: &CftContainer,
    load_output: &ExcelLoadOutput,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_json_model(schema, &load_output.model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, value) in tables {
        let path = dir.join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}

fn write_messagepack_tables(
    schema: &CftContainer,
    load_output: &ExcelLoadOutput,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_messagepack_model(schema, &load_output.model)
        .map_err(|err| format!("failed to export MessagePack model: {err}"))?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, bytes) in tables {
        let path = dir.join(format!("{table}.msgpack"));
        fs::write(&path, bytes)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}
