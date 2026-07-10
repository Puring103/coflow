use coflow_api::{
    CreateTableRequest, DiagnosticSet, ProviderRegistry, SourceLocationSpec, SyncHeaderRequest,
    TableContext,
};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use coflow_runtime::{configured_project_source, DataFileReport};

pub(super) fn infer_table_provider(source: &str) -> Option<&'static str> {
    if source.starts_with("lark:")
        || source.starts_with("https://")
            && (source.contains("feishu") || source.contains("larksuite"))
    {
        Some("lark-sheet")
    } else if std::path::Path::new(source)
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("xlsx")
    {
        Some("excel")
    } else {
        None
    }
}

pub(super) fn create_lark_table(
    session: &coflow_runtime::ProjectSchemaSession,
    registry: &ProviderRegistry,
    source: &str,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<DataFileReport, DiagnosticSet> {
    let source_config = configured_lark_source(session, source)?;
    let resolved_source = configured_project_source(session.project(), source_config);
    let layout = lark_table_layout(session, source_config, actual_type, sheet)?;
    let table_manager = registry.table_manager("lark-sheet").ok_or_else(|| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-PROVIDER",
            "DATA-FILE",
            "lark-sheet table manager is not registered",
        ))
    })?;
    let result = table_manager.create_table(
        TableContext {
            project_root: &session.project().root_dir,
        },
        &CreateTableRequest {
            source: &resolved_source,
            sheet: &layout.sheet,
            actual_type: &layout.actual_type,
            headers: &layout.headers,
        },
    )?;
    Ok(DataFileReport {
        file: source.to_string(),
        provider: "lark-sheet".to_string(),
        sheet: Some(layout.sheet),
        actual_type: Some(layout.actual_type),
        headers: result.headers,
        added: result.added,
        removed: result.removed,
        diagnostics: Vec::new(),
    })
}

pub(super) fn sync_lark_header(
    session: &coflow_runtime::ProjectSchemaSession,
    registry: &ProviderRegistry,
    source: &str,
    actual_type: String,
    sheet: Option<String>,
) -> Result<DataFileReport, DiagnosticSet> {
    let source_config = configured_lark_source(session, source)?;
    let resolved_source = configured_project_source(session.project(), source_config);
    let layout = lark_table_layout(session, source_config, Some(actual_type), sheet)?;
    let table_manager = registry.table_manager("lark-sheet").ok_or_else(|| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-PROVIDER",
            "DATA-FILE",
            "lark-sheet table manager is not registered",
        ))
    })?;
    let result = table_manager.sync_header(
        TableContext {
            project_root: &session.project().root_dir,
        },
        &SyncHeaderRequest {
            source: &resolved_source,
            sheet: Some(&layout.sheet),
            actual_type: &layout.actual_type,
            headers: &layout.headers,
            schema: None,
        },
    )?;
    Ok(DataFileReport {
        file: source.to_string(),
        provider: "lark-sheet".to_string(),
        sheet: Some(layout.sheet),
        actual_type: Some(layout.actual_type),
        headers: result.headers,
        added: result.added,
        removed: result.removed,
        diagnostics: Vec::new(),
    })
}

fn configured_lark_source<'a>(
    session: &'a coflow_runtime::ProjectSchemaSession,
    source: &str,
) -> Result<&'a coflow_project::SourceConfig, DiagnosticSet> {
    session
        .project()
        .config
        .sources
        .iter()
        .find(|candidate| {
            candidate.source_type.as_deref() == Some("lark-sheet")
                && matches!(candidate.location(), SourceLocationSpec::Uri(uri) if uri == source)
        })
        .ok_or_else(|| {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "DATA-FILE-SOURCE",
                "DATA-FILE",
                format!("lark source `{source}` is not configured"),
            ))
        })
}

struct CliTableLayout {
    actual_type: String,
    sheet: String,
    headers: Vec<String>,
}

fn lark_table_layout(
    session: &coflow_runtime::ProjectSchemaSession,
    source: &coflow_project::SourceConfig,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<CliTableLayout, DiagnosticSet> {
    let table_options = lark_table_options(source)?;
    let sheet_config =
        matching_lark_sheet_config(&table_options, actual_type.as_deref(), sheet.as_deref());
    let actual_type = actual_type
        .or_else(|| {
            sheet_config
                .and_then(|config| config.type_name.as_ref())
                .cloned()
        })
        .ok_or_else(|| {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "DATA-FILE-TYPE",
                "DATA-FILE",
                "`--type` is required for lark table creation",
            ))
        })?;
    let schema_view = session.schema_view();
    let Some(schema_fields) = schema_view.full_fields(&actual_type) else {
        return Err(DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-TYPE",
            "DATA-FILE",
            format!("unknown CFT type `{actual_type}`"),
        )));
    };
    if schema_view
        .type_meta(&actual_type)
        .is_some_and(|schema_type| schema_type.is_abstract)
    {
        return Err(DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-TYPE",
            "DATA-FILE",
            format!("abstract type `{actual_type}` cannot be used for a lark table"),
        )));
    }
    let sheet = sheet
        .or_else(|| sheet_config.map(|config| config.sheet.clone()))
        .unwrap_or_else(|| actual_type.clone());
    let key_header = sheet_config
        .and_then(|config| config.key.clone())
        .unwrap_or_else(|| "id".to_string());
    let field_headers = sheet_config
        .map(|config| {
            config
                .columns
                .iter()
                .map(|(source, field)| (field.clone(), source.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut headers = vec![key_header];
    headers.extend(schema_fields.iter().map(|field| {
        field_headers
            .get(&field.name)
            .cloned()
            .unwrap_or_else(|| field.name.clone())
    }));
    Ok(CliTableLayout {
        actual_type,
        sheet,
        headers,
    })
}

fn lark_table_options(
    source: &coflow_project::SourceConfig,
) -> Result<TableSourceOptions, DiagnosticSet> {
    TableSourceOptions::decode(source.options(), "lark source").map_err(|err| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-SOURCE",
            "DATA-FILE",
            err.message,
        ))
    })
}

fn matching_lark_sheet_config<'a>(
    options: &'a TableSourceOptions,
    actual_type: Option<&str>,
    sheet: Option<&str>,
) -> Option<&'a TableSheetConfig> {
    options
        .sheets()
        .iter()
        .find(|config| {
            let type_matches = actual_type
                .is_none_or(|expected| config.type_name.as_deref() == Some(expected));
            let sheet_matches = sheet.is_none_or(|expected| config.sheet == expected);
            type_matches && sheet_matches
        })
}
