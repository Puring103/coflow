use super::{CfdDataModel, CfdRecordHandle, CfdValue};
use crate::diagnostics::RecordOrigin;
use coflow_language::cft::CftSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionFieldLookupError {
    UnknownRecord,
    NotDimensional,
    DimensionMismatch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DimensionValueLookup<'a> {
    Value {
        value: &'a CfdValue,
        origin: &'a RecordOrigin,
    },
    ExplicitNone {
        origin: &'a RecordOrigin,
    },
    Missing,
}

impl CfdDataModel {
    /// 返回原始模型的指定单维上下文有效值；此显式字段查询不继承模型投影选择。
    #[must_use]
    pub fn dimension_value(
        &self,
        source_record: CfdRecordHandle,
        field_name: &str,
        variant: &str,
    ) -> Option<&CfdValue> {
        let source = self.projection.as_ref().map_or(self, |view| &view.source);
        let record = source.record(source_record)?;
        record
            .dimension_field(field_name)
            .and_then(|values| values.effective_variants.get(variant))
            .or_else(|| record.field(field_name))
    }

    /// 判断字段是否显式填写该变体；None、空字符串和与基础值相同的值都算已填写。
    #[must_use]
    pub fn has_dimension_variant(
        &self,
        source_record: CfdRecordHandle,
        field_name: &str,
        variant: &str,
    ) -> bool {
        self.record(source_record)
            .and_then(|record| record.dimension_field(field_name))
            .is_some_and(|values| values.variants.contains_key(variant))
    }

    /// Looks up a dimension-specific value stored on its owning record.
    ///
    /// # Errors
    ///
    /// Returns an error when the owner record, schema field, dimension, or
    /// dimension does not match the requested coordinate.
    pub fn dimension_field_value<'a>(
        &'a self,
        schema: &CftSchema,
        source_record: CfdRecordHandle,
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
        let Some(values) = record.dimension_field(field_name) else {
            return Ok(DimensionValueLookup::Missing);
        };
        if values.dimension.as_str() != dimension {
            return Err(DimensionFieldLookupError::DimensionMismatch);
        }
        let Some(value) = values.variants.get(variant) else {
            return Ok(DimensionValueLookup::Missing);
        };
        if matches!(value.value, CfdValue::OptionNone) {
            Ok(DimensionValueLookup::ExplicitNone {
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
