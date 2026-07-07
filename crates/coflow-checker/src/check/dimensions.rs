use coflow_cft::{CftContainer, CftSchemaView};
use coflow_data_model::{CfdDataModel, CfdErrorCode, CfdPath, CfdRecordId};

use super::diagnostics::dimension_lookup_error_message;
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue};
use crate::DimensionCheckContext;

pub(super) enum DimensionVariantAbort {
    Skipped,
    Error {
        code: CfdErrorCode,
        path: Option<CfdPath>,
        message: String,
    },
}

pub(super) fn apply_dimension_variant(
    schema: &CftSchemaView,
    source_schema: &CftContainer,
    model: &CfdDataModel,
    context: Option<&DimensionCheckContext>,
    record: &CheckRecordRef,
    field_name: &str,
    located: &mut LocatedCheckValue,
) -> Result<Option<CfdRecordId>, DimensionVariantAbort> {
    let Some(context) = context else {
        return Ok(None);
    };
    if !matches!(record, CheckRecordRef::Top(_)) {
        return Ok(None);
    }
    let context_dimension = context.dimension.clone();
    let Some(variant) = context.variant.clone() else {
        return Ok(None);
    };
    let Some(actual_type) = record.actual_type(model) else {
        return Ok(None);
    };
    let Some(field) = schema.dimension_field(actual_type, field_name) else {
        return Ok(None);
    };
    if field.dimension != context_dimension {
        return Ok(None);
    }
    let CheckRecordRef::Top(source_record_id) = record else {
        return Ok(None);
    };
    let resolved = model
        .dimension_field_value(
            source_schema,
            *source_record_id,
            field_name,
            &context_dimension,
            &variant,
        )
        .map_err(|err| DimensionVariantAbort::Error {
            code: CfdErrorCode::CheckEvalTypeError,
            path: located.path.clone(),
            message: dimension_lookup_error_message(actual_type, field_name, &variant, err),
        })?;

    let path = located.path.clone();
    located.value = CheckValue::from_cfd_value_with_path(
        resolved.value,
        resolved.field_type.as_ref(),
        path.clone(),
        model,
        resolved.record,
    );
    if matches!(located.value, CheckValue::Null) {
        return Err(DimensionVariantAbort::Skipped);
    }
    located.path = path;
    Ok(resolved.record)
}
