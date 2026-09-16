use crate::data_model::CfdValue;
use crate::CfdSource;
use coflow_core::schema::{CftField, CftSchema, RecordKey, VariantName};

#[derive(Debug, Clone, Copy)]
pub struct DimensionFieldSchema<'a> {
    pub schema: &'a CftSchema,
    pub source_field: &'a CftField,
}

#[derive(Debug, Clone)]
pub struct WriteDimensionValueRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionFieldSchema<'a>,
    pub actual_type: &'a str,
    pub source_key: &'a RecordKey,
    pub variant: &'a VariantName,
    pub new_value: Option<&'a CfdValue>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DimensionWriteResult {
    pub changed: bool,
}
