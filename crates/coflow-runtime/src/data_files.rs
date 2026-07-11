use coflow_api::{
    CreateTableRequest, Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry,
    ResolvedSource, Severity, SourceLocationSpec, SyncHeaderRequest, TableAddressing, TableContext,
    TableManager, TableManagerDescriptor,
};
use coflow_project::{path_to_slash, Project, SourceConfig};
use serde::Serialize;
use std::path::Path;

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
pub struct TableHeaderLayout {
    pub actual_type: String,
    pub sheet: String,
    pub headers: Vec<String>,
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
    let (provider_id, source) = table_operation_source(
        session.project(),
        registry,
        &options.file,
        options.provider.as_deref(),
    )?;
    let descriptor = table_manager_descriptor(registry, &provider_id)?;
    let manager = table_manager(registry, &provider_id)?;
    let actual_type = options.actual_type;
    let layout = descriptor
        .requires_table_layout()
        .then(|| {
            table_header_layout(
                session,
                manager.as_ref(),
                &source,
                &options.file,
                actual_type.clone(),
                options.sheet,
            )
        })
        .transpose()?;
    let result = manager.create_table(
        table_context(session),
        &CreateTableRequest {
            source: &source,
            sheet: layout.as_ref().map_or("", |layout| layout.sheet.as_str()),
            actual_type: layout.as_ref().map_or_else(
                || actual_type.as_deref().unwrap_or(""),
                |layout| layout.actual_type.as_str(),
            ),
            headers: layout
                .as_ref()
                .map_or_else(|| [].as_slice(), |layout| layout.headers.as_slice()),
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
    let (provider_id, source) = table_operation_source(
        session.project(),
        registry,
        &options.file,
        options.provider.as_deref(),
    )?;
    let descriptor = table_manager_descriptor(registry, &provider_id)?;
    if matches!(&source.location, SourceLocationSpec::Path(path) if !path.exists()) {
        return Err(one_data_file_error(
            "DATA-FILE-MISSING",
            format!("file `{}` does not exist", options.file),
        ));
    }
    let manager = table_manager(registry, &provider_id)?;
    let layout = table_header_layout(
        session,
        manager.as_ref(),
        &source,
        &options.file,
        Some(options.actual_type),
        options.sheet,
    )?;
    let compiled_schema = session.compiled_schema();
    let result = manager.sync_header(
        table_context(session),
        &SyncHeaderRequest {
            source: &source,
            sheet: descriptor
                .requires_table_layout()
                .then_some(layout.sheet.as_str()),
            actual_type: &layout.actual_type,
            headers: &layout.headers,
            schema: Some(&compiled_schema),
        },
    )?;
    Ok(report(
        options.file,
        provider_id,
        descriptor.requires_table_layout().then_some(layout.sheet),
        Some(layout.actual_type),
        result.headers,
        result.added,
        result.removed,
    ))
}

fn table_context(session: &ProjectSchemaSession) -> TableContext<'_> {
    TableContext {
        project_root: &session.project.root_dir,
    }
}

fn table_operation_source(
    project: &Project,
    registry: &ProviderRegistry,
    target: &str,
    requested_provider: Option<&str>,
) -> Result<(String, ResolvedSource), DiagnosticSet> {
    let configured = configured_table_source(project, target);
    if let Some(configured) = configured {
        let requested = requested_provider
            .map(|provider| resolve_explicit_provider_id(registry, provider))
            .transpose()?;
        if let (Some(requested), Some(configured_provider)) =
            (requested.as_deref(), configured.source_type.as_deref())
        {
            if requested != configured_provider {
                return Err(one_data_file_error(
                    "DATA-FILE-PROVIDER",
                    format!(
                        "configured source `{target}` uses provider `{configured_provider}`, not `{requested}`"
                    ),
                ));
            }
        }
        let provider_id = if let Some(requested) = requested {
            requested
        } else if let Some(configured_provider) = &configured.source_type {
            configured_provider.clone()
        } else if matches!(configured.location(), SourceLocationSpec::Path(_)) {
            resolve_provider_id(registry, None, target)?
        } else {
            String::new()
        };
        let mut source = if provider_id.is_empty() {
            crate::configured_project_source(project, registry, configured)?
        } else {
            crate::load::configured_project_source_as(
                project,
                registry,
                configured,
                &provider_id,
            )?
        };
        source.location = match configured.location() {
            SourceLocationSpec::Uri(_) => SourceLocationSpec::Uri(target.to_string()),
            SourceLocationSpec::Path(_) => {
                SourceLocationSpec::Path(project.resolve_path(Path::new(target)))
            }
        };
        source.display_name = target.to_string();
        return Ok((source.provider_id.clone(), source));
    }

    if is_uri_target(target) {
        return Err(one_data_file_error(
            "DATA-FILE-SOURCE",
            format!("remote table source `{target}` is not configured"),
        ));
    }
    let provider_id = resolve_provider_id(registry, requested_provider, target)?;
    let provider = registry.source_provider(&provider_id).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("source provider `{provider_id}` is not registered"),
        )
    })?;
    let source = ResolvedSource {
        provider_id: provider_id.clone(),
        location: SourceLocationSpec::Path(project.resolve_path(Path::new(target))),
        options: provider.decode_options(&serde_json::Value::Null)?,
        display_name: target.to_string(),
    };
    Ok((provider_id, source))
}

fn is_uri_target(target: &str) -> bool {
    let Some((scheme, remainder)) = target.split_once(':') else {
        return false;
    };
    if scheme.len() == 1 && (remainder.starts_with('\\') || remainder.starts_with('/')) {
        return false;
    }
    let mut chars = scheme.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

trait TableManagerDescriptorExt {
    fn requires_table_layout(&self) -> bool;
}

impl TableManagerDescriptorExt for TableManagerDescriptor {
    fn requires_table_layout(&self) -> bool {
        self.addressing == TableAddressing::Sheet
    }
}

fn table_manager(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Result<std::sync::Arc<dyn TableManager>, DiagnosticSet> {
    registry.table_manager(provider_id).ok_or_else(|| {
        one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("table manager `{provider_id}` is not registered"),
        )
    })
}

fn table_manager_descriptor(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Result<&'static TableManagerDescriptor, DiagnosticSet> {
    registry
        .table_manager_descriptors()
        .into_iter()
        .find(|descriptor| descriptor.id == provider_id)
        .ok_or_else(|| {
            one_data_file_error(
                "DATA-FILE-PROVIDER",
                format!("table manager `{provider_id}` is not registered"),
            )
        })
}

const fn report(
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

fn resolve_provider_id(
    registry: &ProviderRegistry,
    raw: Option<&str>,
    file: &str,
) -> Result<String, DiagnosticSet> {
    if let Some(provider) = raw {
        return resolve_explicit_provider_id(registry, provider);
    }
    let extension = Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let candidates = registry
        .table_manager_descriptors()
        .into_iter()
        .filter(|descriptor| descriptor.file_extensions.contains(&extension))
        .map(|descriptor| descriptor.id)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [provider] => Ok((*provider).to_string()),
        [] => Err(one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("cannot infer provider from extension `{extension}` for `{file}`"),
        )),
        _ => Err(one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!(
                "extension `{extension}` for `{file}` matches multiple providers {}; pass --provider",
                candidates.join(", ")
            ),
        )),
    }
}

fn resolve_explicit_provider_id(
    registry: &ProviderRegistry,
    provider: &str,
) -> Result<String, DiagnosticSet> {
    if registry.table_manager(provider).is_some() {
        return Ok(provider.to_string());
    }
    let candidates = registry
        .table_manager_descriptors()
        .into_iter()
        .filter(|descriptor| descriptor.aliases.contains(&provider))
        .map(|descriptor| descriptor.id)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [provider] => Ok((*provider).to_string()),
        [] => Err(one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!("unknown data file provider `{provider}`"),
        )),
        _ => Err(one_data_file_error(
            "DATA-FILE-PROVIDER",
            format!(
                "data file provider alias `{provider}` matches multiple providers {}; pass canonical --provider",
                candidates.join(", ")
            ),
        )),
    }
}

pub fn table_header_layout(
    session: &ProjectSchemaSession,
    manager: &dyn TableManager,
    source: &ResolvedSource,
    file: &str,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<TableHeaderLayout, DiagnosticSet> {
    let actual_type = match actual_type {
        Some(actual_type) => actual_type,
        None => manager.type_for_sheet(source, sheet.as_deref())?.ok_or_else(|| {
            one_data_file_error(
                "DATA-FILE-TYPE",
                format!("`--type` is required for table file `{file}`"),
            )
        })?,
    };
    let compiled_schema = session.compiled_schema();
    let schema_type = compiled_schema.type_meta(&actual_type).ok_or_else(|| {
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
    let sheet = match sheet {
        Some(sheet) => sheet,
        None => manager
            .sheet_for_type(source, &actual_type)?
            .unwrap_or_else(|| actual_type.clone()),
    };
    let header_options = manager.header_options(source, &sheet, &actual_type)?;
    let mut headers = vec![header_options.key_column().to_string()];
    let field_headers = header_options.field_headers();
    headers.extend(
        compiled_schema
            .full_fields(&actual_type)
            .unwrap_or(&[])
            .iter()
            .map(|field| {
                field_headers
                    .get(&field.name)
                    .cloned()
                    .unwrap_or_else(|| field.name.clone())
            }),
    );
    Ok(TableHeaderLayout {
        actual_type,
        sheet: header_options.sheet,
        headers,
    })
}

fn configured_table_source<'a>(project: &'a Project, file: &str) -> Option<&'a SourceConfig> {
    project
        .config
        .sources
        .iter()
        .find(|source| source_location_matches(project, source, file))
}

fn source_location_matches(project: &Project, source: &SourceConfig, file: &str) -> bool {
    let SourceLocationSpec::Path(path) = source.location() else {
        return matches!(source.location(), SourceLocationSpec::Uri(uri) if uri == file);
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
