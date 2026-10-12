use crate::data_model::CfdValue;
use crate::CfdSource;
use coflow_language::cft::{CftDimension, CftField, CftSchema, CftType, RecordKey, VariantName};

#[derive(Debug, Clone, Copy)]
pub struct DimensionSourceSchema<'a> {
    pub schema: &'a CftSchema,
    pub dimension: &'a CftDimension,
    pub source_type: &'a CftType,
    pub source_field: &'a CftField,
}

#[derive(Debug, Clone)]
pub struct WriteDimensionValueRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionSourceSchema<'a>,
    pub source_key: &'a RecordKey,
    pub variant: &'a VariantName,
    pub new_value: Option<&'a CfdValue>,
}

#[derive(Debug, Clone)]
pub struct RewriteDimensionRecordRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: &'a CftSchema,
    pub actual_type: &'a str,
    pub old_key: &'a RecordKey,
    pub new_key: Option<&'a RecordKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DimensionSourceResult {
    pub changed: bool,
}
