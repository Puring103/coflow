use std::collections::BTreeMap;
use std::sync::Arc;

use crate::api::{
    CfdDimensionWriter, CfdDocumentWriter, CfdSource, CfdSourceCatalog, CfdWriteContext,
    DiagnosticSet, DimensionSourceSchema, WriteCellRequest, WriteDimensionValueRequest,
    WriteFieldPathSegment,
};
use crate::data_model::{
    CfdPathSegment, CfdRecordId, CfdValue, DimensionRefCoordinate, RecordOrigin,
};
use coflow_language::{CftSchema, RecordKey};

use super::writer::{lookup_source_writer, source_for_id};
use crate::indexes::SourceId;
use crate::ProjectSession;

pub(super) enum ReferenceUpdateAction {
    Source {
        writer: Arc<dyn CfdDocumentWriter>,
        source: CfdSource,
        requests: Vec<OwnedWriteCellRequest>,
        display_path: String,
    },
    Dimension {
        manager: Arc<dyn CfdDimensionWriter>,
        request: OwnedDimensionWriteRequest,
        display_path: String,
    },
}

impl ReferenceUpdateAction {
    pub(super) const fn source(&self) -> &CfdSource {
        match self {
            Self::Source { source, .. } => source,
            Self::Dimension { request, .. } => &request.source,
        }
    }

    pub(super) const fn writer(&self) -> Option<&Arc<dyn CfdDocumentWriter>> {
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
        model: &crate::data_model::CfdDataModel,
    ) -> Result<DiagnosticSet, DiagnosticSet> {
        match self {
            Self::Source {
                writer,
                source,
                requests,
                ..
            } => {
                let requests = requests
                    .iter()
                    .map(|request| request.as_request(schema, source))
                    .collect::<Vec<_>>();
                writer
                    .write_field_batch(
                        crate::api::WriteContext {
                            project_root,
                            schema,
                            model: Some(model),
                        },
                        &requests,
                    )
                    .map(|outcomes| {
                        let mut diagnostics = DiagnosticSet::empty();
                        for outcome in outcomes {
                            diagnostics.extend(outcome.diagnostics);
                        }
                        diagnostics
                    })
                    .map_err(|failure| failure.diagnostics)
            }
            Self::Dimension {
                manager, request, ..
            } => manager
                .write_dimension_value(
                    CfdWriteContext { project_root },
                    &request.as_request(schema)?,
                )
                .map(|_| DiagnosticSet::empty()),
        }
    }
}

pub(super) struct OwnedDimensionWriteRequest {
    source: CfdSource,
    source_type: coflow_language::TypeName,
    source_field: coflow_language::FieldName,
    dimension: coflow_language::DimensionName,
    variant: coflow_language::VariantName,
    source_key: RecordKey,
    new_value: CfdValue,
}

impl OwnedDimensionWriteRequest {
    fn as_request<'a>(
        &'a self,
        schema: &'a CftSchema,
    ) -> Result<WriteDimensionValueRequest<'a>, DiagnosticSet> {
        let source_type = schema.resolve_type(&self.source_type).ok_or_else(|| {
            transaction_invariant(format!(
                "dimension source type `{}` disappeared before reference rewrite",
                self.source_type
            ))
        })?;
        let source_field = schema
            .field(&self.source_type, &self.source_field)
            .ok_or_else(|| {
                transaction_invariant(format!(
                    "dimension source field `{}.{}` disappeared before reference rewrite",
                    self.source_type, self.source_field
                ))
            })?;
        let dimension = schema.resolve_dimension(&self.dimension).ok_or_else(|| {
            transaction_invariant(format!(
                "dimension `{}` disappeared before reference rewrite",
                self.dimension
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
}

impl OwnedWriteCellRequest {
    pub(super) fn as_request<'a>(
        &'a self,
        schema: &'a CftSchema,
        source: &'a CfdSource,
    ) -> WriteCellRequest<'a> {
        WriteCellRequest {
            origin: &self.origin,
            record_key: &self.record_key,
            actual_type: &self.actual_type,
            field_path: &self.field_path,
            new_value: &self.new_value,
            schema,
            source,
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn reference_update_actions(
    session: &ProjectSession,
    catalog: &CfdSourceCatalog,
    target_id: CfdRecordId,
    new_key: &str,
) -> Result<Vec<ReferenceUpdateAction>, DiagnosticSet> {
    let new_key = RecordKey::new(new_key.to_string()).map_err(|error| {
        transaction_invariant(format!(
            "new record key became invalid before rewrite: {error}"
        ))
    })?;
    let mut actions = Vec::new();
    let mut source_actions = BTreeMap::<SourceId, usize>::new();
    let mut dimension_actions = BTreeMap::<(CfdRecordId, DimensionRefCoordinate), usize>::new();
    for edge in session.model.ref_edges_to_target(target_id) {
        let Some(host_ref) = session.records.get(edge.site.host) else {
            continue;
        };
        let Some(host_record) = session.model.record(edge.site.host) else {
            continue;
        };
        if let Some(dimension) = &edge.site.dimension {
            let action_key = (edge.site.host, dimension.clone());
            let relative_path = edge
                .site
                .path
                .segments
                .strip_prefix(&[CfdPathSegment::Field(dimension.field.to_string())])
                .unwrap_or(&edge.site.path.segments);
            if let Some(index) = dimension_actions.get(&action_key).copied() {
                let ReferenceUpdateAction::Dimension { request, .. } = &mut actions[index] else {
                    unreachable!("dimension action index must point to a dimension write");
                };
                if !replace_ref_value(&mut request.new_value, relative_path, &new_key) {
                    return Err(transaction_invariant(
                        "indexed dimension reference disappeared before rename",
                    ));
                }
                continue;
            }
            let Some(values) = host_record.dimension_field(dimension.field.as_str()) else {
                continue;
            };
            let Some(value) = values.variants.get(&dimension.variant) else {
                continue;
            };
            let mut root = value.value.clone();
            if !replace_ref_value(&mut root, relative_path, &new_key) {
                continue;
            }
            let field = session
                .schema()
                .field(host_record.actual_type(), &dimension.field)
                .ok_or_else(|| {
                    transaction_invariant(format!(
                        "dimension host field `{}.{}` disappeared before reference rewrite",
                        host_record.actual_type(),
                        dimension.field
                    ))
                })?;
            let source_entry = session
                .source_data
                .dimension_source(
                    field.declaring_type.as_str(),
                    field.name.as_str(),
                    dimension.dimension.as_str(),
                )
                .ok_or_else(|| {
                    transaction_invariant(format!(
                        "dimension field `{}.{}` lost its managed source before reference rewrite",
                        field.declaring_type, field.name
                    ))
                })?;
            let manager = catalog.dimension_source_manager();
            let action_index = actions.len();
            actions.push(ReferenceUpdateAction::Dimension {
                manager,
                display_path: source_entry.display_path.clone(),
                request: OwnedDimensionWriteRequest {
                    source: source_entry.source.clone(),
                    source_type: field.declaring_type.clone(),
                    source_field: field.name.clone(),
                    dimension: dimension.dimension.clone(),
                    variant: dimension.variant.clone(),
                    source_key: RecordKey::new(host_record.key().to_string()).map_err(|error| {
                        transaction_invariant(format!(
                            "validated model record key became invalid before reference rewrite: {error}"
                        ))
                    })?,
                    new_value: root,
                },
            });
            dimension_actions.insert(action_key, action_index);
        } else {
            if !matches!(
                host_record.value_at_path(&edge.site.path),
                Some(CfdValue::Ref(_))
            ) {
                continue;
            }
            let source = source_for_id(session, host_ref.source_id)?;
            let request = OwnedWriteCellRequest {
                origin: host_ref.origin.clone(),
                record_key: host_ref.coordinate.key.to_string(),
                actual_type: host_ref.coordinate.actual_type.to_string(),
                field_path: edge.site.path.segments.clone(),
                new_value: CfdValue::Ref(new_key.clone()),
            };
            if let Some(index) = source_actions.get(&host_ref.source_id).copied() {
                let ReferenceUpdateAction::Source { requests, .. } = &mut actions[index] else {
                    unreachable!("source action index must point to a source write");
                };
                requests.push(request);
            } else {
                let writer = lookup_source_writer(catalog, &source)?;
                let action_index = actions.len();
                actions.push(ReferenceUpdateAction::Source {
                    writer,
                    source,
                    display_path: host_ref.display_path.clone(),
                    requests: vec![request],
                });
                source_actions.insert(host_ref.source_id, action_index);
            }
        }
    }
    Ok(actions)
}

fn transaction_invariant(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(crate::api::Diagnostic::error(
        "MUTATION-TXN-INVARIANT",
        "MUTATION",
        message,
    ))
}

fn replace_ref_value(current: &mut CfdValue, path: &[CfdPathSegment], new_key: &RecordKey) -> bool {
    let Some((segment, rest)) = path.split_first() else {
        if matches!(current, CfdValue::Ref(_)) {
            *current = CfdValue::Ref(new_key.clone());
            return true;
        }
        return false;
    };
    let next = match (current, segment) {
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => {
            object.fields.get_mut(field.as_str())
        }
        (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get_mut(*index),
        (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
            .iter_mut()
            .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
            .map(|(_, value)| value),
        _ => None,
    };
    next.is_some_and(|next| replace_ref_value(next, rest, new_key))
}
