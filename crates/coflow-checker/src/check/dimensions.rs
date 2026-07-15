use std::collections::BTreeMap;
use std::sync::Arc;

use coflow_cft::{CftSchemaTypeRef, CftSchema};
use coflow_data_model::{
    CfdDataModel, CfdErrorCode, CfdRecordId, CfdValue, DimensionFieldLookupError,
    DimensionValueLookup,
};
use coflow_structure::{StructuralBudget, TraversalCursor};

use super::diagnostics::dimension_lookup_error_message;
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue, ValueLocation};
use crate::DimensionCheckContext;

#[derive(Debug, Clone)]
pub(super) struct DimensionRoundView {
    variant: Arc<str>,
    fields_by_record: Arc<BTreeMap<CfdRecordId, BTreeMap<String, ProjectedDimensionField>>>,
}

#[derive(Debug, Clone)]
enum ProjectedDimensionField {
    Value {
        field_type: CftSchemaTypeRef,
        traverse_nested: bool,
    },
    ExplicitNull,
    Error {
        message: String,
        traverse_nested: bool,
    },
}

pub(super) struct MaterializedDimensionValue<'a> {
    pub(super) value: &'a CfdValue,
    pub(super) field_type: Option<&'a CftSchemaTypeRef>,
    pub(super) location: ValueLocation,
}

impl DimensionRoundView {
    pub(super) fn compile(
        schema: &CftSchema,
        model: &CfdDataModel,
        context: &DimensionCheckContext,
    ) -> Self {
        let Some(variant) = context.variant.as_deref() else {
            return Self {
                variant: Arc::from(""),
                fields_by_record: Arc::new(BTreeMap::new()),
            };
        };
        let mut fields_by_record = BTreeMap::new();
        for (record_id, record) in model.records() {
            let Some(type_meta) = schema.resolve_type(record.actual_type()) else {
                continue;
            };
            let fields = type_meta
                .all_fields
                .iter()
                .filter(|field| {
                    field
                        .dimension
                        .as_ref()
                        .is_some_and(|dimension| {
                            dimension.dimension.as_str() == context.dimension
                        })
                })
                .map(|field| {
                    let traverse_nested =
                        schema.field_has_nested_checks(record.actual_type(), &field.name);
                    let projection = match model.dimension_field_value(
                        schema,
                        record_id,
                        &field.name,
                        &context.dimension,
                        variant,
                    ) {
                        Ok(DimensionValueLookup::Value { .. }) => {
                            ProjectedDimensionField::Value {
                                field_type: field.ty_ref.clone(),
                                traverse_nested,
                            }
                        }
                        Ok(DimensionValueLookup::ExplicitNull { .. }) => {
                            ProjectedDimensionField::ExplicitNull
                        }
                        Ok(DimensionValueLookup::Missing) => {
                            ProjectedDimensionField::Error {
                                message: dimension_lookup_error_message(
                                    record.actual_type(),
                                    &field.name,
                                    variant,
                                    DimensionFieldLookupError::UnknownVariant,
                                ),
                                traverse_nested,
                            }
                        }
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
                    (field.name.to_string(), projection)
                })
                .collect::<BTreeMap<_, _>>();
            if !fields.is_empty() {
                fields_by_record.insert(record_id, fields);
            }
        }
        Self {
            variant: Arc::from(variant),
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
                | ProjectedDimensionField::ExplicitNull
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
                | ProjectedDimensionField::ExplicitNull
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
        let field_type = match projection {
            ProjectedDimensionField::Value {
                field_type,
                ..
            } => field_type,
            ProjectedDimensionField::ExplicitNull => {
                return Err(DimensionVariantAbort::Skipped)
            }
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
            .record(source_record)
            .and_then(|record| record.dimension_field(field_name))
            .and_then(|values| values.variants.get(self.variant.as_ref()))
            .map(|value| &value.value)
        else {
            return Err(DimensionVariantAbort::Error {
                code: CfdErrorCode::CheckEvalTypeError,
                location: Some(logical_location.clone()),
                message: "dimension overlay value disappeared during check execution".to_string(),
            });
        };
        if matches!(value, CfdValue::Null) {
            return Err(DimensionVariantAbort::Skipped);
        }
        Ok(Some(MaterializedDimensionValue {
            value,
            field_type: Some(field_type),
            location: logical_location.backed_by(super::value::ModelCursor::dimension(
                source_record,
                field_name,
                self.variant.as_ref(),
            )),
        }))
    }
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
        materialized.location.clone(),
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
    Ok(None)
}
