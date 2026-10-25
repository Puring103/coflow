use crate::diagnostics::RecordOrigin;
use crate::model::{CfdDictKey, CfdRecordHandle, CfdValue};
use crate::LoadedFormattedString;
use coflow_language::cft::{DimensionName, FieldName, TypeName, VariantName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordDraft {
    pub(crate) id: String,
    pub(crate) actual_type: TypeName,
    pub(crate) fields: BTreeMap<FieldName, ValueDraft>,
    pub(crate) origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueDraft {
    Value(CfdValue),
    OptionSome(Box<ValueDraft>),
    FormattedString(LoadedFormattedString),
    Object(Box<RecordDraft>),
    PendingRef {
        explicit_type: Option<TypeName>,
        id: String,
    },
    Array(Vec<ValueDraft>),
    Dict(Vec<(CfdDictKey, ValueDraft)>),
}

/// 求值只按需访问记录；全量编译使用切片，增量编译在首次读取时验证输入。
pub(crate) trait RecordDraftSource {
    fn get(&self, index: usize) -> Option<&RecordDraft>;

    fn actual_type(&self, index: usize) -> Option<&str> {
        self.get(index).map(|record| record.actual_type.as_str())
    }

    fn dimension_value(
        &self,
        _record: CfdRecordHandle,
        _field: &FieldName,
        _dimension: &DimensionName,
        _variant: &VariantName,
    ) -> Option<&ValueDraft> {
        None
    }
}

impl RecordDraftSource for [RecordDraft] {
    fn get(&self, index: usize) -> Option<&RecordDraft> {
        <[RecordDraft]>::get(self, index)
    }
}
