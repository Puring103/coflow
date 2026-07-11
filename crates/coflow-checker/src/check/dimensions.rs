use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use coflow_cft::{CftSchemaTypeRef, CompiledSchema};
use coflow_data_model::{
    CfdDataModel, CfdErrorCode, CfdRecordId, CfdValue, DimensionFieldLookupError,
};
use coflow_structure::{StructuralBudget, TraversalCursor};

use super::diagnostics::dimension_lookup_error_message;
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue, ModelCursor, ValueLocation};
use crate::DimensionCheckContext;

#[derive(Debug, Clone)]
pub(super) struct DimensionRoundView {
    fields_by_record: Arc<BTreeMap<CfdRecordId, BTreeMap<String, ProjectedDimensionField>>>,
}

#[derive(Debug, Clone)]
enum ProjectedDimensionField {
    Value {
        storage: ModelCursor,
        field_type: Option<CftSchemaTypeRef>,
        traverse_nested: bool,
    },
    Error {
        message: String,
        traverse_nested: bool,
    },
}

pub(super) struct MaterializedDimensionValue<'a> {
    pub(super) value: &'a CfdValue,
    pub(super) field_type: Option<&'a CftSchemaTypeRef>,
    pub(super) location: ValueLocation,
    pub(super) storage_record: CfdRecordId,
}

impl DimensionRoundView {
    pub(super) fn compile(
        schema: &CompiledSchema,
        model: &CfdDataModel,
        context: &DimensionCheckContext,
    ) -> Self {
        let Some(variant) = context.variant.as_deref() else {
            return Self {
                fields_by_record: Arc::new(BTreeMap::new()),
            };
        };
        let mut fields_by_record = BTreeMap::new();
        for (record_id, record) in model.records() {
            let Some(type_meta) = schema.type_meta(record.actual_type()) else {
                continue;
            };
            let fields = type_meta
                .all_fields
                .iter()
                .filter(|field| {
                    field
                        .dimension
                        .as_ref()
                        .is_some_and(|dimension| dimension.dimension == context.dimension)
                })
                .map(|field| {
                    let traverse_nested =
                        value_type_has_checks(schema, &field.ty_ref, &mut BTreeSet::new());
                    let projection = match model.dimension_field_value(
                        schema,
                        record_id,
                        &field.name,
                        &context.dimension,
                        variant,
                    ) {
                        Ok(resolved) => resolved.record.map_or_else(
                            || ProjectedDimensionField::Error {
                                message: dimension_lookup_error_message(
                                    record.actual_type(),
                                    &field.name,
                                    variant,
                                    DimensionFieldLookupError::MissingStorageRecord,
                                ),
                                traverse_nested,
                            },
                            |storage_record| ProjectedDimensionField::Value {
                                storage: ModelCursor::root(storage_record).field(variant),
                                field_type: resolved.field_type,
                                traverse_nested,
                            },
                        ),
                        Err(error) => ProjectedDimensionField::Error {
                            message: dimension_lookup_error_message(
                                record.actual_type(),
                                &field.name,
                                variant,
                                error,
                            ),
                            traverse_nested,
                        },
                    };
                    (field.name.clone(), projection)
                })
                .collect::<BTreeMap<_, _>>();
            if !fields.is_empty() {
                fields_by_record.insert(record_id, fields);
            }
        }
        Self {
            fields_by_record: Arc::new(fields_by_record),
        }
    }

    pub(super) fn errors_for(&self, record: CfdRecordId) -> impl Iterator<Item = (&str, &str)> {
        self.fields_by_record
            .get(&record)
            .into_iter()
            .flat_map(|fields| fields.iter())
            .filter_map(|(field, projection)| match projection {
                ProjectedDimensionField::Error {
                    message,
                    traverse_nested: true,
                } => Some((field.as_str(), message.as_str())),
                ProjectedDimensionField::Value { .. }
                | ProjectedDimensionField::Error {
                    traverse_nested: false,
                    ..
                } => None,
            })
    }

    pub(super) fn field_names(&self, record: CfdRecordId) -> impl Iterator<Item = &str> {
        self.fields_by_record
            .get(&record)
            .into_iter()
            .flat_map(|fields| fields.iter())
            .filter_map(|(field, projection)| match projection {
                ProjectedDimensionField::Value {
                    traverse_nested: true,
                    ..
                }
                | ProjectedDimensionField::Error {
                    traverse_nested: true,
                    ..
                } => Some(field.as_str()),
                ProjectedDimensionField::Value {
                    traverse_nested: false,
                    ..
                }
                | ProjectedDimensionField::Error {
                    traverse_nested: false,
                    ..
                } => None,
            })
    }

    pub(super) fn materialize<'a>(
        &'a self,
        model: &'a CfdDataModel,
        source_record: CfdRecordId,
        field_name: &str,
        logical_location: &ValueLocation,
    ) -> Result<Option<MaterializedDimensionValue<'a>>, DimensionVariantAbort> {
        let Some(projection) = self
            .fields_by_record
            .get(&source_record)
            .and_then(|fields| fields.get(field_name))
        else {
            return Ok(None);
        };
        let (storage, field_type) = match projection {
            ProjectedDimensionField::Value {
                storage,
                field_type,
                ..
            } => (storage, field_type),
            ProjectedDimensionField::Error {
                traverse_nested: true,
                ..
            } => return Err(DimensionVariantAbort::Skipped),
            ProjectedDimensionField::Error {
                message,
                traverse_nested: false,
            } => {
                return Err(DimensionVariantAbort::Error {
                    code: CfdErrorCode::CheckEvalTypeError,
                    location: Some(logical_location.clone()),
                    message: message.clone(),
                });
            }
        };
        let Some(value) = model
            .record(storage.record)
            .and_then(|record| record.value_at_path(&storage.path))
        else {
            return Err(DimensionVariantAbort::Error {
                code: CfdErrorCode::CheckEvalTypeError,
                location: Some(logical_location.clone()),
                message: "dimension storage value disappeared during check execution".to_string(),
            });
        };
        if matches!(value, CfdValue::Null) {
            return Err(DimensionVariantAbort::Skipped);
        }
        Ok(Some(MaterializedDimensionValue {
            value,
            field_type: field_type.as_ref(),
            location: logical_location.backed_by(storage.clone()),
            storage_record: storage.record,
        }))
    }
}

fn value_type_has_checks(
    schema: &CompiledSchema,
    ty: &CftSchemaTypeRef,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        CftSchemaTypeRef::Named(type_name) => schema
            .concrete_assignable_types(type_name)
            .unwrap_or_else(|| vec![type_name.clone()])
            .iter()
            .any(|actual_type| type_has_checks(schema, actual_type, visiting)),
        CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
            value_type_has_checks(schema, inner, visiting)
        }
        CftSchemaTypeRef::Dict(_, value) => value_type_has_checks(schema, value, visiting),
        CftSchemaTypeRef::Int
        | CftSchemaTypeRef::Float
        | CftSchemaTypeRef::Bool
        | CftSchemaTypeRef::String
        | CftSchemaTypeRef::Ref(_) => false,
    }
}

fn type_has_checks(
    schema: &CompiledSchema,
    type_name: &str,
    visiting: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(type_name.to_string()) {
        return false;
    }
    let has_checks = schema.check_schedule(type_name, None).next().is_some()
        || schema.type_meta(type_name).is_some_and(|meta| {
            meta.all_fields
                .iter()
                .any(|field| value_type_has_checks(schema, &field.ty_ref, visiting))
        });
    visiting.remove(type_name);
    has_checks
}

pub(super) enum DimensionVariantAbort {
    Skipped,
    Error {
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: String,
    },
}

pub(super) fn apply_dimension_variant(
    model: &CfdDataModel,
    round: Option<&DimensionRoundView>,
    record: &CheckRecordRef,
    field_name: &str,
    located: &mut LocatedCheckValue,
    budget: &mut StructuralBudget,
) -> Result<Option<CfdRecordId>, DimensionVariantAbort> {
    let Some(round) = round else {
        return Ok(None);
    };
    let Some(source_record_id) = record.top_record_id() else {
        return Ok(None);
    };
    let Some(logical_location) = located.location.as_ref() else {
        return Ok(None);
    };
    let Some(materialized) =
        round.materialize(model, source_record_id, field_name, logical_location)?
    else {
        return Ok(None);
    };
    located.value = CheckValue::from_cfd_value(
        materialized.value,
        materialized.field_type,
        Some(materialized.location.clone()),
        model,
        budget,
        TraversalCursor::root(),
    )
    .map_err(|exceeded| DimensionVariantAbort::Error {
        code: CfdErrorCode::CheckBudgetExceeded,
        location: exceeded.location,
        message: exceeded.error.to_string(),
    })?;
    located.location = Some(materialized.location);
    Ok(Some(materialized.storage_record))
}
