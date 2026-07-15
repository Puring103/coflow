use std::sync::Arc;

use coflow_api::{
    DiagnosticSet, DimensionSourceManager, DimensionSourceSchema, ProviderRegistry, ResolvedSource,
    RewriteRecordReferencesRequest, SourceWriter, SpreadRewriteTarget, TableContext,
    WriteCellRequest, WriteDimensionValueRequest, WriteFieldPathSegment,
};
use coflow_cft::{CftSchema, RecordKey};
use coflow_data_model::{CfdPathSegment, CfdRecordId, CfdValue, RecordOrigin};

use super::writer::{lookup_source_writer, source_for_id};
use crate::ProjectSession;

pub(super) enum ReferenceUpdateAction {
    Source {
        writer: Arc<dyn SourceWriter>,
        request: OwnedWriteCellRequest,
        display_path: String,
    },
    Dimension {
        manager: Arc<dyn DimensionSourceManager>,
        request: OwnedDimensionWriteRequest,
        display_path: String,
    },
}

impl ReferenceUpdateAction {
    pub(super) const fn source(&self) -> &ResolvedSource {
        match self {
            Self::Source { request, .. } => &request.source,
            Self::Dimension { request, .. } => &request.source,
        }
    }

    pub(super) const fn writer(&self) -> Option<&Arc<dyn SourceWriter>> {
        match self {
            Self::Source { writer, .. } => Some(writer),
            Self::Dimension { .. } => None,
        }
    }

    pub(super) fn display_path(&self) -> &str {
        match self {
            Self::Source { display_path, .. } | Self::Dimension { display_path, .. } => {
                display_path
            }
        }
    }

    pub(super) fn execute(
        &self,
        project_root: &std::path::Path,
        schema: &CftSchema,
        model: &coflow_data_model::CfdDataModel,
    ) -> Result<DiagnosticSet, DiagnosticSet> {
        match self {
            Self::Source {
                writer, request, ..
            } => writer
                .write_field(
                    coflow_api::WriteContext {
                        project_root,
                        schema,
                        model: Some(model),
                    },
                    &request.as_request(schema),
                )
                .map(|outcome| outcome.diagnostics),
            Self::Dimension {
                manager, request, ..
            } => manager
                .write_dimension_value(
                    TableContext { project_root },
                    &request.as_request(schema)?,
                )
                .map(|_| DiagnosticSet::empty()),
        }
    }
}

pub(super) struct OwnedDimensionWriteRequest {
    source: ResolvedSource,
    source_type: coflow_cft::TypeName,
    source_field: coflow_cft::FieldName,
    dimension: coflow_cft::DimensionName,
    variant: coflow_cft::VariantName,
    source_key: RecordKey,
    new_value: CfdValue,
}

impl OwnedDimensionWriteRequest {
    fn as_request<'a>(
        &'a self,
        schema: &'a CftSchema,
    ) -> Result<WriteDimensionValueRequest<'a>, DiagnosticSet> {
        let source_type = schema.resolve_type(&self.source_type).ok_or_else(|| {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "WRITE-DIMENSION-SCHEMA",
                "WRITE",
                format!("unknown dimension source type `{}`", self.source_type),
            ))
        })?;
        let source_field = schema
            .field(&self.source_type, &self.source_field)
            .ok_or_else(|| {
                DiagnosticSet::one(coflow_api::Diagnostic::error(
                    "WRITE-DIMENSION-SCHEMA",
                    "WRITE",
                    format!(
                        "unknown dimension source field `{}.{}`",
                        self.source_type, self.source_field
                    ),
                ))
            })?;
        let dimension = schema.resolve_dimension(&self.dimension).ok_or_else(|| {
            DiagnosticSet::one(coflow_api::Diagnostic::error(
                "WRITE-DIMENSION-SCHEMA",
                "WRITE",
                format!("unknown dimension `{}`", self.dimension),
            ))
        })?;
        Ok(WriteDimensionValueRequest {
            source: &self.source,
            schema: DimensionSourceSchema {
                schema,
                dimension,
                source_type,
                source_field,
            },
            source_key: &self.source_key,
            variant: &self.variant,
            new_value: Some(&self.new_value),
        })
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
    pub(super) fn as_request<'a>(&'a self, schema: &'a CftSchema) -> WriteCellRequest<'a> {
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
    display_path: String,
}

impl SourceRewriteAction {
    pub(super) const fn source(&self) -> &ResolvedSource {
        &self.request.source
    }

    pub(super) fn display_path(&self) -> &str {
        &self.display_path
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
        schema: &'a CftSchema,
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
        let Some(host_ref) = session.records.get(edge.site.host) else {
            continue;
        };
        let Some(host_record) = session.model.record(edge.site.host) else {
            continue;
        };
        if let Some(dimension) = &edge.site.dimension {
            let Some(values) = host_record.dimension_field(dimension.field.as_str()) else {
                continue;
            };
            let Some(value) = values.variants.get(&dimension.variant) else {
                continue;
            };
            let mut root = value.value.clone();
            let relative_path = edge
                .site
                .path
                .segments
                .strip_prefix(&[CfdPathSegment::Field(dimension.field.to_string())])
                .unwrap_or(&edge.site.path.segments);
            if !replace_ref_value(&mut root, relative_path, new_key) {
                continue;
            }
            let field = session
                .schema()
                .field(host_record.actual_type(), &dimension.field)
                .ok_or_else(|| {
                    dimension_ref_error(
                        "WRITE-DIMENSION-SCHEMA",
                        format!(
                            "unknown dimension host field `{}.{}`",
                            host_record.actual_type(),
                            dimension.field
                        ),
                    )
                })?;
            let source_entry = session
                .source_data
                .dimension_source(
                    field.declaring_type.as_str(),
                    field.name.as_str(),
                    dimension.dimension.as_str(),
                )
                .ok_or_else(|| {
                    dimension_ref_error(
                        "WRITE-DIMENSION-SOURCE",
                        format!(
                            "dimension field `{}.{}` has no managed source",
                            field.declaring_type, field.name
                        ),
                    )
                })?;
            let manager = registry
                .dimension_source_manager(&source_entry.provider_id)
                .ok_or_else(|| {
                    dimension_ref_error(
                        "WRITE-DIMENSION-PROVIDER",
                        format!(
                            "dimension source provider `{}` is not registered",
                            source_entry.provider_id
                        ),
                    )
                })?;
            actions.push(ReferenceUpdateAction::Dimension {
                manager,
                display_path: source_entry.display_path.clone(),
                request: OwnedDimensionWriteRequest {
                    source: source_entry.source.clone(),
                    source_type: field.declaring_type.clone(),
                    source_field: field.name.clone(),
                    dimension: dimension.dimension.clone(),
                    variant: dimension.variant.clone(),
                    source_key: RecordKey::new(host_record.key().to_string())
                        .map_err(|error| {
                            dimension_ref_error("WRITE-DIMENSION-KEY", error.to_string())
                        })?,
                    new_value: root,
                },
            });
        } else {
            if !matches!(
                host_record.value_at_path(&edge.site.path),
                Some(CfdValue::Ref(_))
            ) {
                continue;
            }
            let source = source_for_id(session, host_ref.source_id)?;
            let writer = lookup_source_writer(registry, &source)?;
            actions.push(ReferenceUpdateAction::Source {
                writer,
                display_path: host_ref.display_path.clone(),
                request: OwnedWriteCellRequest {
                    origin: host_ref.origin.clone(),
                    record_key: host_ref.coordinate.key.clone(),
                    actual_type: host_ref.coordinate.actual_type.clone(),
                    field_path: edge.site.path.segments.clone(),
                    new_value: CfdValue::Ref(new_key.to_string()),
                    source,
                },
            });
        }
    }
    Ok(actions)
}

fn dimension_ref_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(coflow_api::Diagnostic::error(code, "WRITE", message))
}

fn replace_ref_value(current: &mut CfdValue, path: &[CfdPathSegment], new_key: &str) -> bool {
    let Some((segment, rest)) = path.split_first() else {
        if matches!(current, CfdValue::Ref(_)) {
            *current = CfdValue::Ref(new_key.to_string());
            return true;
        }
        return false;
    };
    let next = match (current, segment) {
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => object.fields.get_mut(field),
        (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get_mut(*index),
        (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
            .iter_mut()
            .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
            .map(|(_, value)| value),
        _ => None,
    };
    next.is_some_and(|next| replace_ref_value(next, rest, new_key))
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
        let source = source_for_id(session, host_ref.source_id)?;
        let target = SpreadRewriteTarget {
            origin: host_ref.origin.clone(),
            record_key: host_ref.coordinate.key.clone(),
            actual_type: host_ref.coordinate.actual_type.clone(),
            object_path: edge.path.segments.clone(),
        };
        by_file
            .entry(host_ref.display_path.clone())
            .and_modify(|(_, targets)| targets.push(target.clone()))
            .or_insert_with(|| (source, vec![target]));
    }
    by_file
        .into_iter()
        .map(|(display_path, (source, targets))| {
            let writer = lookup_source_writer(registry, &source)?;
            Ok(SourceRewriteAction {
                writer,
                display_path,
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
