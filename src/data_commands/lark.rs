use coflow_api::{
    CreateTableRequest, DiagnosticSet, ProviderRegistry, SourceLocationSpec, SyncHeaderRequest,
    TableContext, TableManager,
};
use coflow_runtime::{configured_project_source, table_header_layout, DataFileReport};
use std::sync::Arc;

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
    let table_manager = lark_table_manager(registry)?;
    let layout = table_header_layout(
        session,
        table_manager.as_ref(),
        &resolved_source,
        source,
        actual_type,
        sheet,
    )?;
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
    let table_manager = lark_table_manager(registry)?;
    let layout = table_header_layout(
        session,
        table_manager.as_ref(),
        &resolved_source,
        source,
        Some(actual_type),
        sheet,
    )?;
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

fn lark_table_manager(
    registry: &ProviderRegistry,
) -> Result<Arc<dyn TableManager>, DiagnosticSet> {
    registry.table_manager("lark-sheet").ok_or_else(|| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-PROVIDER",
            "DATA-FILE",
            "lark-sheet table manager is not registered",
        ))
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

