use coflow_model::{
    CfdDataModel, CfdDiagnostic, CfdErrorCode, CfdRecordId, CfdValue, DimensionFieldLookupError,
    DimensionValueLookup,
};
use coflow_language::limits::{StructuralBudget, TraversalCursor};
use coflow_language::{CftSchema, CftValueType, DimensionName, VariantName};

use crate::diagnostics::dimension_lookup_error_message;
use crate::eval::{EvalRecordRef, EvalValue, LocatedEvalValue, ValueLocation};
use crate::CheckProjection;

pub(crate) fn attach_dimension_origins(
    model: &CfdDataModel,
    projection: &CheckProjection,
    diagnostic: &mut CfdDiagnostic,
) {
    if let Some(primary) = &mut diagnostic.primary {
        attach_dimension_origin(model, projection, primary);
    }
    for related in &mut diagnostic.related {
        attach_dimension_origin(model, projection, related);
    }
}

fn attach_dimension_origin(
    model: &CfdDataModel,
    projection: &CheckProjection,
    label: &mut coflow_model::CfdLabel,
) {
    let Some((dimension, variant)) = projection.dimension() else {
        return;
    };
    let Some(record) = label.record.and_then(|record| model.record(record)) else {
        return;
    };
    let Some(field) = label
        .path
        .segments
        .iter()
        .find_map(|segment| match segment {
            coflow_model::CfdPathSegment::Field(field) => Some(field.as_str()),
            coflow_model::CfdPathSegment::Index(_)
            | coflow_model::CfdPathSegment::DictKey(_) => None,
        })
    else {
        return;
    };
    let Some(values) = record
        .dimension_field(field)
        .filter(|values| &values.dimension == dimension)
    else {
        return;
    };
    label.origin = values
        .variants
        .get(variant)
        .map(|value| value.origin.clone());
}

#[derive(Debug, Clone)]
pub(crate) struct CheckProjectionView {
    dimension: DimensionName,
    variant: VariantName,
}

enum ProjectedDimensionField {
    Value(CftValueType),
    ExplicitNone,
    Error {
        message: String,
        traverse_nested: bool,
    },
}

pub(crate) struct MaterializedDimensionValue<'a> {
    pub(crate) value: &'a CfdValue,
    pub(crate) field_type: Option<CftValueType>,
    pub(crate) location: ValueLocation,
}

impl CheckProjectionView {
    pub(crate) fn new(projection: &CheckProjection) -> Option<Self> {
        let (dimension, variant) = projection.dimension()?;
        Some(Self {
            dimension: dimension.clone(),
            variant: variant.clone(),
        })
    }

    fn project_field(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        record_id: CfdRecordId,
        field_name: &str,
    ) -> Option<ProjectedDimensionField> {
        let record = model.record(record_id)?;
        let field = schema.field(record.actual_type(), field_name)?;
        if field
            .dimension
            .as_ref()
            .is_none_or(|dimension| dimension.dimension != self.dimension)
        {
            return None;
        }
        let traverse_nested = schema.field_has_nested_checks(record.actual_type(), &field.name);
        Some(
            match model.dimension_field_value(
                schema,
                record_id,
                &field.name,
                &self.dimension,
                &self.variant,
            ) {
                Ok(DimensionValueLookup::Value { .. }) => {
                    ProjectedDimensionField::Value(field.value_type.clone())
                }
                Ok(DimensionValueLookup::ExplicitNone { .. }) => {
                    ProjectedDimensionField::ExplicitNone
                }
                Ok(DimensionValueLookup::Missing) => ProjectedDimensionField::Error {
                    message: dimension_lookup_error_message(
                        record.actual_type(),
                        &field.name,
                        &self.variant,
                        DimensionFieldLookupError::UnknownVariant,
                    ),
                    traverse_nested,
                },
                Err(error) => ProjectedDimensionField::Error {
                    message: dimension_lookup_error_message(
                        record.actual_type(),
                        &field.name,
                        &self.variant,
                        error,
                    ),
                    traverse_nested,
                },
            },
        )
    }

    pub(crate) fn materialize<'model>(
        &self,
        schema: &CftSchema,
        model: &'model CfdDataModel,
        source_record: CfdRecordId,
        field_name: &str,
        logical_location: &ValueLocation,
    ) -> Result<Option<MaterializedDimensionValue<'model>>, DimensionVariantAbort> {
        let Some(projection) = self.project_field(schema, model, source_record, field_name) else {
            return Ok(None);
        };
        let field_type = match projection {
            ProjectedDimensionField::Value(field_type) => field_type,
            ProjectedDimensionField::ExplicitNone => {
                return Err(DimensionVariantAbort::Skipped);
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
                    location: Box::new(Some(logical_location.clone())),
                    message,
                });
            }
        };
        let Some(value) = model
            .record(source_record)
            .and_then(|record| record.dimension_field(field_name))
            .and_then(|values| values.variants.get(&self.variant))
            .map(|value| &value.value)
        else {
            return Err(DimensionVariantAbort::Error {
                code: CfdErrorCode::CheckEvalTypeError,
                location: Box::new(Some(logical_location.clone())),
                message: "dimension overlay value disappeared during check execution".to_string(),
            });
        };
        if matches!(value, CfdValue::OptionNone) {
            return Err(DimensionVariantAbort::Skipped);
        }
        Ok(Some(MaterializedDimensionValue {
            value,
            field_type: Some(field_type),
            location: logical_location.backed_by(crate::eval::ModelCursor::dimension(
                source_record,
                field_name,
                self.variant.as_str(),
            )),
        }))
    }
}

pub(crate) enum DimensionVariantAbort {
    Skipped,
    Error {
        code: CfdErrorCode,
        location: Box<Option<ValueLocation>>,
        message: String,
    },
}

pub(crate) fn apply_dimension_variant<'model>(
    schema: &CftSchema,
    model: &'model CfdDataModel,
    projection: Option<&CheckProjectionView>,
    record: &EvalRecordRef,
    field_name: &str,
    located: &mut LocatedEvalValue<'model>,
    budget: &mut StructuralBudget,
) -> Result<Option<CfdRecordId>, DimensionVariantAbort> {
    let Some(view) = projection else {
        return Ok(None);
    };
    let Some(source_record_id) = record.top_record_id() else {
        return Ok(None);
    };
    let Some(logical_location) = located.location.as_ref() else {
        return Ok(None);
    };
    let Some(materialized) = view.materialize(
        schema,
        model,
        source_record_id,
        field_name,
        logical_location,
    )?
    else {
        return Ok(None);
    };
    located.value = EvalValue::from_cfd_value(
        materialized.value,
        materialized.field_type.as_ref(),
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
