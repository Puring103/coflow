use coflow_api::{
    CreateTableRequest, Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry,
    ResolvedSource, Severity, SourceLocationSpec, SyncHeaderRequest, TableContext,
};
use coflow_cft::CftSchemaView;
use coflow_project::{path_to_slash, Project, SourceConfig};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ProjectSchemaSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataCreateFileOptions {
    pub file: String,
    pub actual_type: Option<String>,
    pub provider: Option<String>,
    pub sheet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSyncHeaderOptions {
    pub file: String,
    pub actual_type: String,
    pub provider: Option<String>,
    pub sheet: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataFileReport {
    pub file: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub actual_type: Option<String>,
    pub headers: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone)]
struct TableLayout {
    actual_type: String,
    sheet: String,
    headers: Vec<String>,
}

/// Creates a local data file for a configured project.
///
/// # Errors
///
/// Returns diagnostics when the provider, schema type, or target file is
/// invalid, or when the file cannot be created.
pub fn create_data_file(
    session: &ProjectSchemaSession,
    registry: &ProviderRegistry,
    options: DataCreateFileOptions,
) -> Result<DataFileReport, DiagnosticSet> {
    let provider_id = resolve_provider_id(options.provider.as_deref(), &options.file)?;
    let path = resolve_project_file(&session.project, &options.file);
    let actual_type = options.actual_type;
    let layout = (provider_id != "cfd")
        .then(|| table_layout(session, &options.file, actual_type.clone(), options.sheet))
        .transpose()?;
    let source = table_operation_source(&options.file, &provider_id, path);
    let result = table_manager(registry, &provider_id)?.create_table(
        table_context(session),
        &CreateTableRequest {
            source: &source,
            sheet: layout.as_ref().map_or("", |layout| layout.sheet.as_str()),
            actual_type: layout
                .as_ref()
                .map_or(actual_type.as_deref().unwrap_or(""), |layout| {
                    layout.actual_type.as_str()
                }),
            headers: layout
                .as_ref()
                .map_or_else(|| [].as_slice(), |layout| layout.headers.as_slice()),
            schema: &session.schema,
        },
    )?;
    Ok(report(
        options.file,
        provider_id,
        layout.as_ref().map(|layout| layout.sheet.clone()),
        layout
            .as_ref()
            .map(|layout| layout.actual_type.clone())
            .or(actual_type),
        result.headers,
        result.added,
        result.removed,
    ))
}

/// Synchronizes a local data file's top-level columns with the latest schema.
///
/// # Errors
///
/// Returns diagnostics when the provider, schema type, or target file is
/// invalid, or when the file cannot be updated.
pub fn sync_data_header(
    session: &ProjectSchemaSession,
    registry: &ProviderRegistry,
    options: DataSyncHeaderOptions,
) -> Result<DataFileReport, DiagnosticSet> {
    let provider_id = resolve_provider_id(options.provider.as_deref(), &options.file)?;
    let path = resolve_project_file(&session.project, &options.file);
    if !path.exists() {
        return Err(one_data_file_error(
            "DATA-FILE-MISSING",
            format!("file `{}` does not exist", options.file),
        ));
    }
    let layout = table_layout(
        session,
        &options.file,
        Some(options.actual_type),
        options.sheet,
    )?;
    let source = table_operation_source(&options.file, &provider_id, path);
    let result = table_manager(registry, &provider_id)?.sync_header(
        table_context(session),
        &SyncHeaderRequest {
            source: &source,
            sheet: (provider_id != "cfd").then_some(layout.sheet.as_str()),
            actual_type: &layout.actual_type,
            headers: &layout.headers,
            schema: &session.schema,
        },
    )?;
    Ok(report(
        options.file,
        provider_id,
        (source.provider_id != "cfd").then_some(layout.sheet),
        Some(layout.actual_type),
        result.headers,
        result.added,
        result.removed,
    ))
}

fn table_context(session: &ProjectSchemaSession) -> TableContext<'_> {
    TableContext {
        project_root: &session.project.root_dir,
        schema: Some(&session.schema),
    }
}

fn table_operation_source(
    file: &str,
    provider_id: &str,
    path: PathBuf,
) -> ResolvedSource {
    ResolvedSource {
        provider_id: provider_id.to_string(),
        location: SourceLocationSpec::Path(path),
        options: Value::Null,
        display_name: file.to_string(),
    }
}

fn table_manager(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Result<std::sync::Arc<dyn coflow_api::TableManager>, DiagnosticSet> {
    registry.table_manager(provider_id).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("table manager `{provider_id}` is not registered"),
        )
    })
}

fn report(
    file: String,
    provider_id: String,
    sheet: Option<String>,
    actual_type: Option<String>,
    headers: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
) -> DataFileReport {
    DataFileReport {
        file,
        provider: provider_id,
        sheet,
        actual_type,
        headers,
        added,
        removed,
        diagnostics: Vec::new(),
    }
}

fn resolve_provider_id(raw: Option<&str>, file: &str) -> Result<String, DiagnosticSet> {
    if let Some(provider) = raw {
        return match provider {
            "cfd" | "csv" | "excel" => Ok(provider.to_string()),
            "xlsx" => Ok("excel".to_string()),
            other => Err(one_data_file_error(
                "DATA-FILE-PROVIDER",
                format!("unknown data file provider `{other}`"),
            )),
        };
    }
    let extension = Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    match extension {
        "cfd" | "csv" => Ok(extension.to_string()),
        "xlsx" => Ok("excel".to_string()),
        other => Err(one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("cannot infer provider from extension `{other}` for `{file}`"),
        )),
    }
}

fn resolve_project_file(project: &Project, file: &str) -> PathBuf {
    project.resolve_path(Path::new(file))
}

fn table_layout(
    session: &ProjectSchemaSession,
    file: &str,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<TableLayout, DiagnosticSet> {
    let actual_type = actual_type
        .or_else(|| configured_type_for_file(&session.project, file, sheet.as_deref()))
        .ok_or_else(|| {
            one_data_file_error(
                "DATA-FILE-TYPE",
                format!("`--type` is required for table file `{file}`"),
            )
        })?;
    let schema_view = CftSchemaView::new(&session.schema);
    let schema_type = schema_view.types.get(&actual_type).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-TYPE",
            format!("unknown CFT type `{actual_type}`"),
        )
    })?;
    if schema_type.is_abstract {
        return Err(one_data_file_error(
            "DATA-FILE-TYPE",
            format!("abstract type `{actual_type}` cannot be used for a data file"),
        ));
    }
    let config =
        table_source_config_for_file(&session.project, file, &actual_type, sheet.as_deref());
    let sheet = sheet
        .or_else(|| config.as_ref().map(|config| config.sheet.clone()))
        .unwrap_or_else(|| actual_type.clone());
    let key_header = config
        .as_ref()
        .and_then(|config| config.key.clone())
        .unwrap_or_else(|| "id".to_string());
    let field_headers = config
        .as_ref()
        .map(|config| config.field_headers.clone())
        .unwrap_or_default();
    let mut headers = vec![key_header];
    headers.extend(schema_type.all_fields.iter().map(|field| {
        field_headers
            .get(&field.name)
            .cloned()
            .unwrap_or_else(|| field.name.clone())
    }));
    Ok(TableLayout {
        actual_type,
        sheet,
        headers,
    })
}

#[derive(Debug, Clone, Default)]
struct SourceTableConfig {
    sheet: String,
    key: Option<String>,
    field_headers: BTreeMap<String, String>,
}

fn configured_type_for_file(project: &Project, file: &str, sheet: Option<&str>) -> Option<String> {
    table_source_config(project, file, None, sheet)
        .and_then(|config| source_sheet_value(&config, "type"))
}

fn table_source_config_for_file(
    project: &Project,
    file: &str,
    actual_type: &str,
    sheet: Option<&str>,
) -> Option<SourceTableConfig> {
    let value = table_source_config(project, file, Some(actual_type), sheet)?;
    let sheet_name = source_sheet_value(&value, "sheet")?;
    let key = source_sheet_value(&value, "key");
    let field_headers = value
        .get("columns")
        .and_then(Value::as_object)
        .map(|columns| {
            columns
                .iter()
                .filter_map(|(source, field)| {
                    field
                        .as_str()
                        .map(|field| (field.to_string(), source.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SourceTableConfig {
        sheet: sheet_name,
        key,
        field_headers,
    })
}

fn table_source_config(
    project: &Project,
    file: &str,
    actual_type: Option<&str>,
    sheet: Option<&str>,
) -> Option<Value> {
    let source = project
        .config
        .sources
        .iter()
        .filter(|source| source_path_matches(project, source, file))
        .find_map(|source| matching_sheet_config(source, actual_type, sheet))?;
    Some(source)
}

fn matching_sheet_config(
    source: &SourceConfig,
    actual_type: Option<&str>,
    sheet: Option<&str>,
) -> Option<Value> {
    let sheets = source.options().get("sheets")?.as_array()?;
    sheets
        .iter()
        .filter_map(Value::as_object)
        .find(|object| {
            let type_matches = actual_type.is_none_or(|expected| {
                object
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate == expected)
            });
            let sheet_matches = sheet.is_none_or(|expected| {
                object
                    .get("sheet")
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate == expected)
            });
            type_matches && sheet_matches
        })
        .map(|object| Value::Object(object.clone()))
}

fn source_sheet_value(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn source_path_matches(project: &Project, source: &SourceConfig, file: &str) -> bool {
    let SourceLocationSpec::Path(path) = source.location() else {
        return false;
    };
    let requested = path_to_slash(Path::new(file));
    let configured = path_to_slash(path);
    if configured == requested {
        return true;
    }
    let absolute = project.resolve_path(path);
    if absolute.is_dir() {
        let requested_path = Path::new(file);
        return requested_path.starts_with(path);
    }
    false
}

fn one_data_file_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: "DATA-FILE".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}
