use std::sync::Arc;

use coflow_api::{
    DiagnosticSet, ProviderRegistry, RecordOrigin, ResolvedSource, RewriteRecordReferencesRequest,
    SourceWriter, SpreadRewriteTarget, WriteCellRequest, WriteFieldPathSegment,
};
use coflow_data_model::{CfdRecordId, CfdValue};

use super::{lookup_source_writer, source_for_file};
use crate::writes::path::{cfd_path_to_write_path, value_at_path, write_path_from_cfd_path};
use crate::ProjectSession;

pub(super) struct ReferenceUpdateAction {
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) request: OwnedWriteCellRequest,
}

impl ReferenceUpdateAction {
    pub(super) fn source(&self) -> &ResolvedSource {
        &self.request.source
    }
}

pub(super) struct OwnedWriteCellRequest {
    origin: RecordOrigin,
    record_key: String,
    actual_type: String,
    field_path: Vec<WriteFieldPathSegment>,
    new_value: CfdValue,
    source: ResolvedSource,
}

impl OwnedWriteCellRequest {
    pub(super) fn as_request<'a>(
        &'a self,
        schema: &'a coflow_api::CftContainer,
    ) -> WriteCellRequest<'a> {
        WriteCellRequest {
            origin: &self.origin,
            record_key: &self.record_key,
            actual_type: &self.actual_type,
            field_path: &self.field_path,
            new_value: &self.new_value,
            schema,
            source: &self.source,
        }
    }
}

pub(super) struct SourceRewriteAction {
    pub(super) writer: Arc<dyn SourceWriter>,
    pub(super) request: OwnedRewriteRecordReferencesRequest,
}

impl SourceRewriteAction {
    pub(super) fn source(&self) -> &ResolvedSource {
        &self.request.source
    }
}

pub(super) struct OwnedRewriteRecordReferencesRequest {
    source: ResolvedSource,
    old_key: String,
    new_key: String,
    targets: Vec<SpreadRewriteTarget>,
}

impl OwnedRewriteRecordReferencesRequest {
    pub(super) fn as_request<'a>(
        &'a self,
        schema: &'a coflow_api::CftContainer,
    ) -> RewriteRecordReferencesRequest<'a> {
        RewriteRecordReferencesRequest {
            source: &self.source,
            old_key: &self.old_key,
            new_key: &self.new_key,
            targets: &self.targets,
            schema,
        }
    }
}

pub(super) fn reference_update_actions(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    target_id: CfdRecordId,
    new_key: &str,
) -> Result<Vec<ReferenceUpdateAction>, DiagnosticSet> {
    let mut actions = Vec::new();
    for edge in session.model.direct_ref_edges_to_target(target_id) {
        let Some(host_ref) = session.records.get(edge.host) else {
            continue;
        };
        let Some(host_record) = session.model.record(edge.host) else {
            continue;
        };
        if !matches!(
            value_at_path(host_record, &edge.path),
            Some(CfdValue::Ref(_))
        ) {
            continue;
        }
        let source = source_for_file(session, &host_ref.display_path)?;
        let writer = lookup_source_writer(registry, &source)?;
        actions.push(ReferenceUpdateAction {
            writer,
            request: OwnedWriteCellRequest {
                origin: host_ref.origin.clone(),
                record_key: host_ref.coordinate.key.clone(),
                actual_type: host_ref.coordinate.actual_type.clone(),
                field_path: write_path_from_cfd_path(&edge.path)?,
                new_value: CfdValue::Ref(new_key.to_string()),
                source,
            },
        });
    }
    Ok(actions)
}

pub(super) fn source_rewrite_actions(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    target_id: CfdRecordId,
    old_key: &str,
    new_key: &str,
) -> Result<Vec<SourceRewriteAction>, DiagnosticSet> {
    let mut by_file =
        std::collections::BTreeMap::<String, (ResolvedSource, Vec<SpreadRewriteTarget>)>::new();
    for edge in session.model.spread_edges_from_source(target_id) {
        let Some(host_ref) = session.records.get(edge.host) else {
            continue;
        };
        let source = source_for_file(session, &host_ref.display_path)?;
        let target = SpreadRewriteTarget {
            origin: host_ref.origin.clone(),
            record_key: host_ref.coordinate.key.clone(),
            actual_type: host_ref.coordinate.actual_type.clone(),
            object_path: cfd_path_to_write_path(&edge.path),
        };
        by_file
            .entry(host_ref.display_path.clone())
            .and_modify(|(_, targets)| targets.push(target.clone()))
            .or_insert_with(|| (source, vec![target]));
    }
    by_file
        .into_values()
        .map(|(source, targets)| {
            let writer = lookup_source_writer(registry, &source)?;
            Ok(SourceRewriteAction {
                writer,
                request: OwnedRewriteRecordReferencesRequest {
                    source,
                    old_key: old_key.to_string(),
                    new_key: new_key.to_string(),
                    targets,
                },
            })
        })
        .collect()
}
