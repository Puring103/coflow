use super::{CfdDataModel, CfdRecordId, CfdValue};
use crate::origin::RecordOrigin;
use coflow_cft::CftSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionFieldLookupError {
    UnknownRecord,
    NotDimensional,
    DimensionMismatch,
    UnknownVariant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DimensionValueLookup<'a> {
    Value {
        value: &'a CfdValue,
        origin: &'a RecordOrigin,
    },
    ExplicitNull {
        origin: &'a RecordOrigin,
    },
    Missing,
}

impl CfdDataModel {
    /// Looks up a dimension-specific value stored on its owning record.
    ///
    /// # Errors
    ///
    /// Returns an error when the owner record, schema field, dimension, or
    /// configured variant does not match the requested coordinate.
    pub fn dimension_field_value<'a>(
        &'a self,
        schema: &CftSchema,
        source_record: CfdRecordId,
        field_name: &str,
        dimension: &str,
        variant: &str,
    ) -> Result<DimensionValueLookup<'a>, DimensionFieldLookupError> {
        let record = self
            .record(source_record)
            .ok_or(DimensionFieldLookupError::UnknownRecord)?;
        let field = schema
            .field(record.actual_type(), field_name)
            .ok_or(DimensionFieldLookupError::NotDimensional)?;
        let binding = field
            .dimension
            .as_ref()
            .ok_or(DimensionFieldLookupError::NotDimensional)?;
        if binding.dimension.as_str() != dimension {
            return Err(DimensionFieldLookupError::DimensionMismatch);
        }
        let schema_dimension = schema
            .resolve_dimension(dimension)
            .ok_or(DimensionFieldLookupError::DimensionMismatch)?;
        if schema_dimension.variant(variant).is_none() {
            return Err(DimensionFieldLookupError::UnknownVariant);
        }
        let Some(values) = record.dimension_field(field_name) else {
            return Ok(DimensionValueLookup::Missing);
        };
        if values.dimension.as_str() != dimension {
            return Err(DimensionFieldLookupError::DimensionMismatch);
        }
        let Some(value) = values.variants.get(variant) else {
            return Ok(DimensionValueLookup::Missing);
        };
        if matches!(value.value, CfdValue::Null) {
            Ok(DimensionValueLookup::ExplicitNull {
                origin: &value.origin,
            })
        } else {
            Ok(DimensionValueLookup::Value {
                value: &value.value,
                origin: &value.origin,
            })
        }
    }
}
