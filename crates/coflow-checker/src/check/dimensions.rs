use coflow_cft::CompiledSchema;
use coflow_data_model::{CfdDataModel, CfdErrorCode, CfdRecordId};

use super::diagnostics::dimension_lookup_error_message;
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue, ModelCursor, ValueLocation};
use crate::DimensionCheckContext;

pub(super) enum DimensionVariantAbort {
    Skipped,
    Error {
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: String,
    },
}

pub(super) fn apply_dimension_variant(
    schema: &CompiledSchema,
    model: &CfdDataModel,
    context: Option<&DimensionCheckContext>,
    record: &CheckRecordRef,
    field_name: &str,
    located: &mut LocatedCheckValue,
) -> Result<Option<CfdRecordId>, DimensionVariantAbort> {
    let Some(context) = context else {
        return Ok(None);
    };
    let Some(source_record_id) = record.top_record_id() else {
        return Ok(None);
    };
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
    let resolved = model
        .dimension_field_value(
            schema,
            source_record_id,
            field_name,
            &context_dimension,
            &variant,
        )
        .map_err(|err| DimensionVariantAbort::Error {
            code: CfdErrorCode::CheckEvalTypeError,
            location: located.location.clone(),
            message: dimension_lookup_error_message(actual_type, field_name, &variant, err),
        })?;

    let location = match (&located.location, resolved.record) {
        (Some(location), Some(record)) => {
            Some(location.backed_by(ModelCursor::root(record).field(&variant)))
        }
        _ => located.location.clone(),
    };
    located.value = CheckValue::from_cfd_value(
        resolved.value,
        resolved.field_type.as_ref(),
        location.clone(),
        model,
    );
    if matches!(located.value, CheckValue::Null) {
        return Err(DimensionVariantAbort::Skipped);
    }
    located.location = location;
    Ok(resolved.record)
}
