use crate::diagnostics::RecordOrigin;
use crate::model::{CfdDictKey, CfdValue};
use crate::schema::{FieldName, TypeName};
use crate::CallableSource;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordDraft {
    pub(crate) key: String,
    pub(crate) actual_type: TypeName,
    pub(crate) fields: BTreeMap<FieldName, ValueDraft>,
    pub(crate) origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueDraft {
    Value(CfdValue),
    OptionSome(Box<ValueDraft>),
    FormattedString(CallableSource),
    Object(Box<RecordDraft>),
    PendingRef {
        expected_type: TypeName,
        required_type: TypeName,
        key: String,
    },
    Array(Vec<ValueDraft>),
    Dict(Vec<(CfdDictKey, ValueDraft)>),
}
