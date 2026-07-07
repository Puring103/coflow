use coflow_api::{
    CreateTableRequest, DiagnosticSet, ProviderRegistry, SourceLocationSpec, WriteContext,
};
use coflow_engine::{configured_project_source, DataFileReport};

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
    session: &coflow_engine::ProjectSchemaSession,
    registry: &ProviderRegistry,
    source: &str,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<DataFileReport, DiagnosticSet> {
    let source_config = session
        .project
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
        })?;
    let resolved_source = configured_project_source(&session.project, source_config);
    let layout = lark_table_layout(session, source_config, actual_type, sheet)?;
    let writer = registry.writer("lark-sheet").ok_or_else(|| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-PROVIDER",
            "DATA-FILE",
            "lark-sheet writer is not registered",
        ))
    })?;
    writer.create_table(
        WriteContext {
            project_root: &session.project.root_dir,
            schema: &session.schema,
            model: None,
        },
        &CreateTableRequest {
            source: &resolved_source,
            sheet: &layout.sheet,
            actual_type: &layout.actual_type,
            headers: &layout.headers,
            schema: &session.schema,
        },
    )?;
    Ok(DataFileReport {
        file: source.to_string(),
        provider: "lark-sheet".to_string(),
        sheet: Some(layout.sheet),
        actual_type: Some(layout.actual_type),
        headers: layout.headers,
        added: Vec::new(),
        removed: Vec::new(),
        diagnostics: Vec::new(),
    })
}

struct CliTableLayout {
    actual_type: String,
    sheet: String,
    headers: Vec<String>,
}

fn lark_table_layout(
    session: &coflow_engine::ProjectSchemaSession,
    source: &coflow_project::SourceConfig,
    actual_type: Option<String>,
    sheet: Option<String>,
) -> Result<CliTableLayout, DiagnosticSet> {
    let sheet_config = matching_lark_sheet_config(source, actual_type.as_deref(), sheet.as_deref());
    let actual_type = actual_type
        .or_else(|| {
            sheet_config
                .as_ref()
                .and_then(|config| config.get("type"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "DATA-FILE-TYPE",
                "DATA-FILE",
                "`--type` is required for lark table creation",
            ))
        })?;
    let schema_type = session.schema.resolve_type(&actual_type).ok_or_else(|| {
        DiagnosticSet::one(coflow_api::Diagnostic::error(
            "DATA-FILE-TYPE",
            "DATA-FILE",
            format!("unknown CFT type `{actual_type}`"),
        ))
    })?;
    let sheet = sheet
        .or_else(|| {
            sheet_config
                .as_ref()
                .and_then(|config| config.get("sheet"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| actual_type.clone());
    let key_header = sheet_config
        .as_ref()
        .and_then(|config| config.get("key"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("id")
        .to_string();
    let field_headers = sheet_config
        .as_ref()
        .and_then(|config| config.get("columns"))
        .and_then(serde_json::Value::as_object)
        .map(|columns| {
            columns
                .iter()
                .filter_map(|(source, field)| {
                    field
                        .as_str()
                        .map(|field| (field.to_string(), source.clone()))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut headers = vec![key_header];
    headers.extend(schema_type.all_fields.iter().map(|field| {
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

fn matching_lark_sheet_config(
    source: &coflow_project::SourceConfig,
    actual_type: Option<&str>,
    sheet: Option<&str>,
) -> Option<serde_json::Value> {
    source
        .options()
        .get("sheets")?
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_object)
        .find(|object| {
            let type_matches = actual_type.is_none_or(|expected| {
                object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|candidate| candidate == expected)
            });
            let sheet_matches = sheet.is_none_or(|expected| {
                object
                    .get("sheet")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|candidate| candidate == expected)
            });
            type_matches && sheet_matches
        })
        .map(|object| serde_json::Value::Object(object.clone()))
}
