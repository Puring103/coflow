use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry, WriterCapabilities};
use coflow_data_model::CfdValue;
use serde::{Deserialize, Serialize};

use crate::{ProjectSession, RecordCoordinate};

const DEFAULT_GET_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct DataSourcesReport {
    pub sources: Vec<DataSourceInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataSourceInfo {
    pub file: String,
    pub provider: String,
    pub capabilities: WriterCapabilities,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataListQuery {
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub limit: Option<usize>,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataListReport {
    pub records: Vec<DataRecordSummary>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataRecordSummary {
    pub record: RecordCoordinate,
    pub file: String,
    pub provider: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataGetQuery {
    pub selector: Option<RecordCoordinate>,
    pub actual_type: Option<String>,
    pub file: Option<String>,
    pub keys: Vec<String>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub all: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataGetReport {
    pub records: Vec<DataRecordInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataRecordInfo {
    pub record: RecordCoordinate,
    pub file: String,
    pub provider: String,
    pub fields: BTreeMap<String, CfdValue>,
}

#[must_use]
pub fn data_sources(session: &ProjectSession, registry: &ProviderRegistry) -> DataSourcesReport {
    let sources = session
        .sources
        .entries()
        .iter()
        .map(|entry| {
            let types = session
                .records
                .ids_in_file(&entry.display_path)
                .iter()
                .filter_map(|id| session.records.get(*id))
                .map(|record_ref| record_ref.coordinate.actual_type.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            DataSourceInfo {
                file: entry.display_path.clone(),
                provider: entry.provider_id.clone(),
                capabilities: writer_capabilities(registry, &entry.provider_id),
                types,
            }
        })
        .collect();

    DataSourcesReport {
        sources,
        diagnostics: flat_diagnostics(session),
    }
}

#[must_use]
pub fn data_list(session: &ProjectSession, query: &DataListQuery) -> DataListReport {
    let records = record_summaries(session, query.file.as_deref(), query.actual_type.as_deref());
    let records = paginate(&records, query.offset, query.limit);

    DataListReport {
        records,
        diagnostics: flat_diagnostics(session),
    }
}

/// Returns full records matching the query.
///
/// # Errors
///
/// Returns diagnostics when an explicit selector cannot be found or when an
/// unbounded query would return more than the default safety limit.
pub fn data_get(
    session: &ProjectSession,
    query: &DataGetQuery,
) -> Result<DataGetReport, DiagnosticSet> {
    let mut summaries = selected_summaries(session, query)?;
    apply_key_filter(&mut summaries, &query.keys);

    if requires_explicit_large_get(query, summaries.len()) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "DATA-GET-LIMIT",
            "DATA",
            format!(
                "data get matched {} records before pagination; pass --limit or --all to fetch this many records (--offset alone is not enough)",
                summaries.len()
            ),
        )));
    }

    let limit = if query.all {
        None
    } else {
        Some(query.limit.unwrap_or(DEFAULT_GET_LIMIT))
    };
    let records = paginate(&summaries, query.offset, limit)
        .into_iter()
        .map(|summary| {
            let view = session
                .record_view(&summary.record.actual_type, &summary.record.key)
                .ok_or_else(|| DiagnosticSet::one(not_found(&summary.record)))?;
            Ok(DataRecordInfo {
                record: summary.record,
                file: summary.file,
                provider: summary.provider,
                fields: view.record.fields().clone(),
            })
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()?;

    Ok(DataGetReport {
        records,
        diagnostics: flat_diagnostics(session),
    })
}

fn selected_summaries(
    session: &ProjectSession,
    query: &DataGetQuery,
) -> Result<Vec<DataRecordSummary>, DiagnosticSet> {
    if let Some(selector) = &query.selector {
        let view = session
            .record_view(&selector.actual_type, &selector.key)
            .ok_or_else(|| DiagnosticSet::one(not_found(selector)))?;
        if !matches_query_filters(&view.coordinate, view.display_path, query) {
            return Ok(Vec::new());
        }
        return Ok(vec![DataRecordSummary {
            record: view.coordinate,
            file: view.display_path.to_string(),
            provider: view.provider_id.to_string(),
        }]);
    }

    Ok(record_summaries(
        session,
        query.file.as_deref(),
        query.actual_type.as_deref(),
    ))
}

fn matches_query_filters(coordinate: &RecordCoordinate, file: &str, query: &DataGetQuery) -> bool {
    query
        .actual_type
        .as_ref()
        .is_none_or(|actual_type| coordinate.actual_type == *actual_type)
        && query
            .file
            .as_ref()
            .is_none_or(|filter_file| file == filter_file)
}

fn record_summaries(
    session: &ProjectSession,
    file: Option<&str>,
    actual_type: Option<&str>,
) -> Vec<DataRecordSummary> {
    if let Some(file) = file {
        return record_summaries_in_file(session, file, actual_type);
    }

    session
        .files
        .source_files()
        .iter()
        .flat_map(|file| record_summaries_in_file(session, file, actual_type))
        .collect()
}

fn record_summaries_in_file(
    session: &ProjectSession,
    file: &str,
    actual_type: Option<&str>,
) -> Vec<DataRecordSummary> {
    session
        .records
        .ids_in_file(file)
        .iter()
        .filter_map(|id| session.records.get(*id))
        .filter(|record_ref| actual_type.is_none_or(|ty| record_ref.coordinate.actual_type == ty))
        .map(|record_ref| DataRecordSummary {
            record: record_ref.coordinate.clone(),
            file: record_ref.display_path.clone(),
            provider: record_ref.provider_id.clone(),
        })
        .collect()
}

fn apply_key_filter(summaries: &mut Vec<DataRecordSummary>, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    let keys = keys.iter().map(String::as_str).collect::<BTreeSet<_>>();
    summaries.retain(|summary| keys.contains(summary.record.key.as_str()));
}

fn paginate<T: Clone>(items: &[T], offset: usize, limit: Option<usize>) -> Vec<T> {
    if offset >= items.len() {
        return Vec::new();
    }
    let end = limit
        .map_or(items.len(), |limit| offset.saturating_add(limit))
        .min(items.len());
    items[offset..end].to_vec()
}

const fn requires_explicit_large_get(query: &DataGetQuery, match_count: usize) -> bool {
    query.selector.is_none()
        && !query.all
        && query.limit.is_none()
        && match_count > DEFAULT_GET_LIMIT
}

fn writer_capabilities(registry: &ProviderRegistry, provider_id: &str) -> WriterCapabilities {
    registry.source_writer(provider_id).map_or_else(
        || WriterCapabilities::read_only().with_provider_id(provider_id.to_string()),
        |writer| {
            writer
                .descriptor()
                .capabilities
                .clone()
                .with_provider_id(provider_id.to_string())
        },
    )
}

fn not_found(coordinate: &RecordCoordinate) -> Diagnostic {
    Diagnostic::error(
        "DATA-NOT-FOUND",
        "DATA",
        format!(
            "record `{}.{}` was not found",
            coordinate.actual_type, coordinate.key
        ),
    )
}

fn flat_diagnostics(session: &ProjectSession) -> Vec<FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let location = session.diagnostics.logical_location(index);
            let actual_type = location.and_then(|l| l.actual_type.clone());
            let record_key = location.and_then(|l| l.record_key.clone());
            let field_path = location.and_then(|l| l.field_path.clone());
            diagnostic.flat_view(actual_type, record_key, field_path)
        })
        .collect()
}
