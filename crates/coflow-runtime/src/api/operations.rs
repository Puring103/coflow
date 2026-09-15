use crate::data_model::{CfdValue, DimensionValueDraft};
use crate::CfdSource;
use coflow_core::schema::{CftDimension, CftField, CftSchema, CftType, RecordKey, VariantName};
use std::sync::Arc;
#[derive(Debug, Clone)]
pub struct DimensionSourceRequest<'a> {
    pub schema: &'a CftSchema,
    pub source: &'a CfdSource,
    pub entries: &'a [DimensionSourceEntry],
    pub variants: &'a [String],
}

#[derive(Debug, Clone, Copy)]
pub struct DimensionSourceSchema<'a> {
    pub schema: &'a CftSchema,
    pub dimension: &'a CftDimension,
    pub source_type: &'a CftType,
    pub source_field: &'a CftField,
}

#[derive(Debug, Clone)]
pub struct DimensionSourceLoadRequest<'a> {
    pub source: &'a CfdSource,
    pub schema: DimensionSourceSchema<'a>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DimensionSourceLoadResult {
    pub values: Vec<DimensionValueDraft>,
    pub source: Arc<str>,
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
    pub schema: DimensionSourceSchema<'a>,
    pub old_key: &'a RecordKey,
    pub new_key: Option<&'a RecordKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DimensionSourceEntry {
    pub key: String,
    pub actual_type: String,
    pub default: CfdValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DimensionSourceResult {
    pub changed: bool,
}
