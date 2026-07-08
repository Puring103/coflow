use super::{CfdDataModel, CfdRecordId, CfdValue};
use crate::compiler_context::DataModelCompilerContext;
use coflow_cft::{CftContainer, CftSchemaTypeRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionFieldLookupError {
    NotDimensional,
    DimensionMismatch,
    MissingStorageRecord,
    MissingVariantField,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionFieldValue<'a> {
    pub value: &'a CfdValue,
    pub record: Option<CfdRecordId>,
    pub field_type: Option<CftSchemaTypeRef>,
}

impl CfdDataModel {
    /// Looks up a dimension-specific value for a source record field.
    ///
    /// # Errors
    ///
    /// Returns an error when the source field is not dimensional, the caller
    /// asks for a different dimension, the generated storage record is missing,
    /// or the requested variant field is not present on that storage record.
    pub fn dimension_field_value<'a>(
        &'a self,
        schema: &CftContainer,
        source_record: CfdRecordId,
        field_name: &str,
        dimension: &str,
        variant: &str,
    ) -> Result<DimensionFieldValue<'a>, DimensionFieldLookupError> {
        let record = self
            .record(source_record)
            .ok_or(DimensionFieldLookupError::MissingStorageRecord)?;
        let actual_type = record.actual_type();
        let compiler_context = DataModelCompilerContext::new(schema);
        let source_type = compiler_context
            .type_meta(actual_type)
            .ok_or(DimensionFieldLookupError::NotDimensional)?;
        let field = compiler_context
            .field_meta(actual_type, field_name)
            .ok_or(DimensionFieldLookupError::NotDimensional)?;
        let Some(field_dimension) = field
            .dimension
            .as_ref()
            .map(|dimension| dimension.dimension.as_str())
        else {
            return Err(DimensionFieldLookupError::NotDimensional);
        };
        if field_dimension != dimension {
            return Err(DimensionFieldLookupError::DimensionMismatch);
        }
        let storage_type = compiler_context
            .dimension_storage_type(dimension, actual_type, field_name)
            .ok_or(DimensionFieldLookupError::MissingStorageRecord)?;
        let storage_key = if source_type.is_singleton {
            field_name
        } else {
            record.key()
        };
        let storage_id = self
            .lookup_assignable(storage_type, storage_key)
            .ok_or(DimensionFieldLookupError::MissingStorageRecord)?;
        let storage_record = self
            .record(storage_id)
            .ok_or(DimensionFieldLookupError::MissingStorageRecord)?;
        let value = storage_record
            .field(variant)
            .ok_or(DimensionFieldLookupError::MissingVariantField)?;
        let field_type = compiler_context
            .field_meta(storage_type, variant)
            .map(|field| field.ty_ref.clone());
        Ok(DimensionFieldValue {
            value,
            record: Some(storage_id),
            field_type,
        })
    }
}
