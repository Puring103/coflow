use coflow_cft::{CftSchema, CftValueType, DimensionName, VariantName};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdErrorCode, CfdRecordId, CfdValue, DimensionFieldLookupError,
    DimensionValueLookup,
};
use coflow_structure::{StructuralBudget, TraversalCursor};

use crate::diagnostics::dimension_lookup_error_message;
use crate::eval::{EvalRecordRef, EvalValue, LocatedEvalValue, ValueLocation};
use crate::DimensionCheckContext;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DimensionCheckRound {
    pub(crate) dimension: DimensionName,
    pub(crate) variant: VariantName,
}

impl DimensionCheckRound {
    /// Creates a round only when the dimension and variant belong to `schema`.
    ///
    /// # Errors
    ///
    /// Returns [`DimensionCheckRoundError`] for an unknown dimension or a
    /// variant that is not declared by that dimension.
    pub fn try_new(
        schema: &CftSchema,
        dimension: DimensionName,
        variant: VariantName,
    ) -> Result<Self, DimensionCheckRoundError> {
        let Some(schema_dimension) = schema.resolve_dimension(&dimension) else {
            return Err(DimensionCheckRoundError::UnknownDimension(dimension));
        };
        if schema_dimension.variant(&variant).is_none() {
            return Err(DimensionCheckRoundError::UnknownVariant { dimension, variant });
        }
        Ok(Self { dimension, variant })
    }

    #[must_use]
    pub const fn dimension(&self) -> &DimensionName {
        &self.dimension
    }

    #[must_use]
    pub const fn variant(&self) -> &VariantName {
        &self.variant
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DimensionCheckRoundError {
    UnknownDimension(DimensionName),
    UnknownVariant {
        dimension: DimensionName,
        variant: VariantName,
    },
}

impl std::fmt::Display for DimensionCheckRoundError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDimension(dimension) => {
                write!(formatter, "unknown check dimension `{dimension}`")
            }
            Self::UnknownVariant { dimension, variant } => write!(
                formatter,
                "unknown check variant `{variant}` for dimension `{dimension}`"
            ),
        }
    }
}

impl std::error::Error for DimensionCheckRoundError {}

pub(crate) fn attach_dimension_origins(
    model: &CfdDataModel,
    round: &DimensionCheckRound,
    diagnostic: &mut CfdDiagnostic,
) {
    if let Some(primary) = &mut diagnostic.primary {
        attach_dimension_origin(model, round, primary);
    }
    for related in &mut diagnostic.related {
        attach_dimension_origin(model, round, related);
    }
}

fn attach_dimension_origin(
    model: &CfdDataModel,
    round: &DimensionCheckRound,
    label: &mut coflow_data_model::CfdLabel,
) {
    let Some(record) = label.record.and_then(|record| model.record(record)) else {
        return;
    };
    let Some(field) = label
        .path
        .segments
        .iter()
        .find_map(|segment| match segment {
            coflow_data_model::CfdPathSegment::Field(field) => Some(field.as_str()),
            coflow_data_model::CfdPathSegment::Index(_)
            | coflow_data_model::CfdPathSegment::DictKey(_) => None,
        })
    else {
        return;
    };
    let Some(values) = record
        .dimension_field(field)
        .filter(|values| values.dimension == round.dimension)
    else {
        return;
    };
    label.origin = values
        .variants
        .get(&round.variant)
        .map(|value| value.origin.clone());
}

#[derive(Debug, Clone)]
pub(crate) struct DimensionRoundView {
    dimension: DimensionName,
    variant: VariantName,
    projected_records: Rc<RefCell<BTreeSet<CfdRecordId>>>,
}

enum ProjectedDimensionField {
    Value(CftValueType),
    ExplicitNull,
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

impl DimensionRoundView {
    pub(crate) fn new(context: &DimensionCheckContext) -> Self {
        Self {
            dimension: context.dimension.clone(),
            variant: context.variant.clone(),
            projected_records: Rc::new(RefCell::new(BTreeSet::new())),
        }
    }

    fn project_field(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        record_id: CfdRecordId,
        field_name: &str,
    ) -> Option<ProjectedDimensionField> {
        self.projected_records.borrow_mut().insert(record_id);
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
                Ok(DimensionValueLookup::ExplicitNull { .. }) => {
                    ProjectedDimensionField::ExplicitNull
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

    pub(crate) fn nested_fields(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        record_id: CfdRecordId,
    ) -> Vec<(String, Option<String>)> {
        self.projected_records.borrow_mut().insert(record_id);
        let Some(record) = model.record(record_id) else {
            return Vec::new();
        };
        let Some(type_meta) = schema.resolve_type(record.actual_type()) else {
            return Vec::new();
        };
        type_meta
            .all_fields()
            .filter(|field| schema.field_has_nested_checks(record.actual_type(), &field.name))
            .filter_map(|field| {
                let projection = self.project_field(schema, model, record_id, &field.name)?;
                match projection {
                    ProjectedDimensionField::Value(_) => Some((field.name.to_string(), None)),
                    ProjectedDimensionField::Error {
                        message,
                        traverse_nested: true,
                    } => Some((field.name.to_string(), Some(message))),
                    ProjectedDimensionField::ExplicitNull
                    | ProjectedDimensionField::Error {
                        traverse_nested: false,
                        ..
                    } => None,
                }
            })
            .collect()
    }

    pub(crate) fn projected_record_count(&self) -> usize {
        self.projected_records.borrow().len()
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
            ProjectedDimensionField::ExplicitNull => {
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
        if matches!(value, CfdValue::Null) {
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
    round: Option<&DimensionRoundView>,
    record: &EvalRecordRef,
    field_name: &str,
    located: &mut LocatedEvalValue<'model>,
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
    let Some(materialized) = round.materialize(
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
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
