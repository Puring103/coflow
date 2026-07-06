use coflow_api::{
    CreateTableRequest, Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry,
    ResolvedSource, Severity, SourceLocationSpec, SyncHeaderRequest, TableContext,
};
use coflow_cfd::{parse_cfd, CfdBlockEntry, CfdRecord as AstRecord};
use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaField, CftSchemaTypeRef, Span};
use coflow_data_model::{CfdEnumValue, CfdObject, CfdValue};
use coflow_loader_cfd::writer::serialize_value;
use coflow_project::{path_to_slash, Project, SourceConfig};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataFileProvider {
    Cfd,
    Csv,
    Excel,
}

impl DataFileProvider {
    const fn id(self) -> &'static str {
        match self {
            Self::Cfd => "cfd",
            Self::Csv => "csv",
            Self::Excel => "excel",
        }
    }
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
    let provider = resolve_provider(options.provider.as_deref(), &options.file)?;
    let path = resolve_project_file(&session.project, &options.file);

    match provider {
        DataFileProvider::Cfd => {
            ensure_new_data_file_path(&path, &options.file)?;
            fs::write(&path, "").map_err(|err| {
                one_data_file_error(
                    "DATA-FILE-IO",
                    format!("failed to write `{}`: {err}", path.display()),
                )
            })?;
            Ok(report(
                options.file,
                provider,
                None,
                options.actual_type,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ))
        }
        DataFileProvider::Csv => {
            let layout = table_layout(session, &options.file, options.actual_type, options.sheet)?;
            let source = table_operation_source(&options.file, provider, path);
            let result = table_manager(registry, provider)?.create_table(
                table_context(session),
                &CreateTableRequest {
                    source: &source,
                    sheet: &layout.sheet,
                    actual_type: &layout.actual_type,
                    headers: &layout.headers,
                    schema: &session.schema,
                },
            )?;
            Ok(report(
                options.file,
                provider,
                Some(layout.sheet),
                Some(layout.actual_type),
                result.headers,
                result.added,
                result.removed,
            ))
        }
        DataFileProvider::Excel => {
            let layout = table_layout(session, &options.file, options.actual_type, options.sheet)?;
            let source = table_operation_source(&options.file, provider, path);
            let result = table_manager(registry, provider)?.create_table(
                table_context(session),
                &CreateTableRequest {
                    source: &source,
                    sheet: &layout.sheet,
                    actual_type: &layout.actual_type,
                    headers: &layout.headers,
                    schema: &session.schema,
                },
            )?;
            Ok(report(
                options.file,
                provider,
                Some(layout.sheet),
                Some(layout.actual_type),
                result.headers,
                result.added,
                result.removed,
            ))
        }
    }
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
    let provider = resolve_provider(options.provider.as_deref(), &options.file)?;
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
    match provider {
        DataFileProvider::Cfd => {
            let old_fields = cfd_top_level_fields(&path, &layout.actual_type)?;
            let added = added_columns(&layout.headers, &old_fields);
            let removed = removed_columns(&layout.headers, &old_fields);
            sync_cfd_columns(&path, &session.schema, &layout.actual_type)?;
            Ok(report(
                options.file,
                provider,
                None,
                Some(layout.actual_type),
                layout.headers,
                added,
                removed,
            ))
        }
        DataFileProvider::Csv => {
            let source = table_operation_source(&options.file, provider, path);
            let result = table_manager(registry, provider)?.sync_header(
                table_context(session),
                &SyncHeaderRequest {
                    source: &source,
                    sheet: Some(&layout.sheet),
                    actual_type: &layout.actual_type,
                    headers: &layout.headers,
                    schema: &session.schema,
                },
            )?;
            Ok(report(
                options.file,
                provider,
                Some(layout.sheet),
                Some(layout.actual_type),
                result.headers,
                result.added,
                result.removed,
            ))
        }
        DataFileProvider::Excel => {
            let source = table_operation_source(&options.file, provider, path);
            let result = table_manager(registry, provider)?.sync_header(
                table_context(session),
                &SyncHeaderRequest {
                    source: &source,
                    sheet: Some(&layout.sheet),
                    actual_type: &layout.actual_type,
                    headers: &layout.headers,
                    schema: &session.schema,
                },
            )?;
            Ok(report(
                options.file,
                provider,
                Some(layout.sheet),
                Some(layout.actual_type),
                result.headers,
                result.added,
                result.removed,
            ))
        }
    }
}

fn table_context(session: &ProjectSchemaSession) -> TableContext<'_> {
    TableContext {
        project_root: &session.project.root_dir,
        schema: &session.schema,
    }
}

fn table_operation_source(
    file: &str,
    provider: DataFileProvider,
    path: PathBuf,
) -> ResolvedSource {
    ResolvedSource {
        provider_id: provider.id().to_string(),
        location: SourceLocationSpec::Path(path),
        options: Value::Null,
        display_name: file.to_string(),
    }
}

fn table_manager(
    registry: &ProviderRegistry,
    provider: DataFileProvider,
) -> Result<std::sync::Arc<dyn coflow_api::TableManager>, DiagnosticSet> {
    registry.table_manager(provider.id()).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("table manager `{}` is not registered", provider.id()),
        )
    })
}

fn ensure_new_data_file_path(path: &Path, file: &str) -> Result<(), DiagnosticSet> {
    if path.exists() {
        return Err(one_data_file_error(
            "DATA-FILE-EXISTS",
            format!("file `{file}` already exists"),
        ));
    }
    ensure_parent_dir(path)
}

fn ensure_parent_dir(path: &Path) -> Result<(), DiagnosticSet> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            one_data_file_error(
                "DATA-FILE-IO",
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
    }
    Ok(())
}

fn report(
    file: String,
    provider: DataFileProvider,
    sheet: Option<String>,
    actual_type: Option<String>,
    headers: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
) -> DataFileReport {
    DataFileReport {
        file,
        provider: provider.id().to_string(),
        sheet,
        actual_type,
        headers,
        added,
        removed,
        diagnostics: Vec::new(),
    }
}

fn resolve_provider(raw: Option<&str>, file: &str) -> Result<DataFileProvider, DiagnosticSet> {
    if let Some(provider) = raw {
        return match provider {
            "cfd" => Ok(DataFileProvider::Cfd),
            "csv" => Ok(DataFileProvider::Csv),
            "excel" | "xlsx" => Ok(DataFileProvider::Excel),
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
        "cfd" => Ok(DataFileProvider::Cfd),
        "csv" => Ok(DataFileProvider::Csv),
        "xlsx" => Ok(DataFileProvider::Excel),
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
    let schema_type = session.schema.resolve_type(&actual_type).ok_or_else(|| {
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

fn added_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let old = old_header.iter().collect::<BTreeSet<_>>();
    new_header
        .iter()
        .filter(|header| !old.contains(header))
        .cloned()
        .collect()
}

fn removed_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let new = new_header.iter().collect::<BTreeSet<_>>();
    old_header
        .iter()
        .filter(|header| !new.contains(header))
        .cloned()
        .collect()
}

fn cfd_top_level_fields(path: &Path, actual_type: &str) -> Result<Vec<String>, DiagnosticSet> {
    let text = fs::read_to_string(path).map_err(|err| {
        one_data_file_error(
            "DATA-FILE-IO",
            format!("failed to read `{}`: {err}", path.display()),
        )
    })?;
    let (ast, diagnostics) = parse_cfd(&text);
    if let Some(diagnostic) = diagnostics.first() {
        return Err(one_data_file_error(
            "DATA-FILE-PARSE",
            format!(
                "failed to parse `{}`: {}",
                path.display(),
                diagnostic.message
            ),
        ));
    }
    let mut fields = BTreeSet::new();
    for record in ast
        .records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        for field in &record.fields {
            fields.insert(field.name.clone());
        }
    }
    let mut out = vec!["id".to_string()];
    out.extend(fields);
    Ok(out)
}

fn sync_cfd_columns(
    path: &Path,
    schema: &CftContainer,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    let text = fs::read_to_string(path).map_err(|err| {
        one_data_file_error(
            "DATA-FILE-IO",
            format!("failed to read `{}`: {err}", path.display()),
        )
    })?;
    let (ast, diagnostics) = parse_cfd(&text);
    if let Some(diagnostic) = diagnostics.first() {
        return Err(one_data_file_error(
            "DATA-FILE-PARSE",
            format!(
                "failed to parse `{}`: {}",
                path.display(),
                diagnostic.message
            ),
        ));
    }
    let schema_type = schema.resolve_type(actual_type).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-TYPE",
            format!("unknown CFT type `{actual_type}`"),
        )
    })?;
    let fields = schema_type
        .all_fields
        .iter()
        .map(|field| (field.name.clone(), field))
        .collect::<BTreeMap<_, _>>();
    let new_text = rewrite_cfd_records(&text, &ast.records, actual_type, schema, &fields)?;
    fs::write(path, new_text).map_err(|err| {
        one_data_file_error(
            "DATA-FILE-IO",
            format!("failed to write `{}`: {err}", path.display()),
        )
    })
}

fn rewrite_cfd_records(
    source: &str,
    records: &[AstRecord],
    actual_type: &str,
    schema: &CftContainer,
    fields: &BTreeMap<String, &CftSchemaField>,
) -> Result<String, DiagnosticSet> {
    let mut replacements = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        replacements.push((
            record.span,
            render_cfd_record(source, record, schema, fields),
        ));
    }
    replace_spans(source, &replacements)
}

fn render_cfd_record(
    source: &str,
    record: &AstRecord,
    schema: &CftContainer,
    fields: &BTreeMap<String, &CftSchemaField>,
) -> String {
    let existing = record
        .fields
        .iter()
        .map(|field| (field.name.clone(), raw_span(source, field.value.span())))
        .collect::<BTreeMap<_, _>>();
    let mut out = format!(
        "{}: {} {{\n",
        format_record_key(&record.key),
        record.type_name
    );
    for entry in &record.entries {
        let CfdBlockEntry::Spread(_, span) = entry else {
            continue;
        };
        out.push_str("  ");
        out.push_str(raw_span(source, *span).trim());
        out.push_str(",\n");
    }
    for (field_name, field) in fields {
        let value = existing
            .get(field_name)
            .cloned()
            .unwrap_or_else(|| default_cfd_value(schema, field));
        out.push_str("  ");
        out.push_str(field_name);
        out.push_str(": ");
        out.push_str(&value);
        out.push_str(",\n");
    }
    out.push_str("}\n");
    out
}

fn raw_span(source: &str, span: Span) -> String {
    source
        .get(span.start..span.end)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn format_record_key(key: &str) -> String {
    if coflow_cft::is_cft_identifier(key) {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

fn default_cfd_value(schema: &CftContainer, field: &CftSchemaField) -> String {
    let value = field.default.as_ref().map_or_else(
        || value_from_type_default(schema, &field.ty_ref),
        |default| value_from_schema_default(schema, &field.ty_ref, default),
    );
    serialize_value(&value, 2)
}

fn value_from_schema_default(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    default: &CftSchemaDefaultValue,
) -> CfdValue {
    match default {
        CftSchemaDefaultValue::Null => CfdValue::Null,
        CftSchemaDefaultValue::Int(value) => CfdValue::Int(*value),
        CftSchemaDefaultValue::Float(value) => CfdValue::Float(*value),
        CftSchemaDefaultValue::Bool(value) => CfdValue::Bool(*value),
        CftSchemaDefaultValue::String(value) => CfdValue::String(value.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => CfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.clone(),
            variant: Some(variant.clone()),
            value: *value,
        }),
        CftSchemaDefaultValue::EmptyArray => CfdValue::Array(Vec::new()),
        CftSchemaDefaultValue::EmptyObject => value_from_type_default(schema, ty),
    }
}

fn value_from_type_default(schema: &CftContainer, ty: &CftSchemaTypeRef) -> CfdValue {
    match ty {
        CftSchemaTypeRef::Int => CfdValue::Int(0),
        CftSchemaTypeRef::Float => CfdValue::Float(0.0),
        CftSchemaTypeRef::Bool => CfdValue::Bool(false),
        CftSchemaTypeRef::String => CfdValue::String(String::new()),
        CftSchemaTypeRef::Ref(_) | CftSchemaTypeRef::Nullable(_) => CfdValue::Null,
        CftSchemaTypeRef::Array(_) => CfdValue::Array(Vec::new()),
        CftSchemaTypeRef::Dict(_, _) => CfdValue::Dict(Vec::new()),
        CftSchemaTypeRef::Named(name) if schema.has_enum(name) => schema
            .resolve_enum(name)
            .and_then(|enm| enm.variants.first())
            .map_or_else(
                || {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: None,
                        value: 0,
                    })
                },
                |variant| {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: Some(variant.name.clone()),
                        value: variant.value,
                    })
                },
            ),
        CftSchemaTypeRef::Named(name) => {
            let fields = schema
                .resolve_type(name)
                .map(|ty| {
                    ty.all_fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                value_from_type_default(schema, &field.ty_ref),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            CfdValue::Object(Box::new(CfdObject::new(name.clone(), fields)))
        }
    }
}

fn replace_spans(source: &str, replacements: &[(Span, String)]) -> Result<String, DiagnosticSet> {
    let mut out = source.to_string();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|(span, _)| span.start);
    for (span, _) in &sorted {
        if span.start > source.len() || span.end > source.len() || span.start > span.end {
            return Err(one_data_file_error(
                "DATA-FILE-PARSE",
                format!(
                    "span [{}, {}) is out of bounds for source of length {}",
                    span.start,
                    span.end,
                    source.len()
                ),
            ));
        }
    }
    for (span, replacement) in sorted.into_iter().rev() {
        out.replace_range(span.start..span.end, &replacement);
    }
    Ok(out)
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
