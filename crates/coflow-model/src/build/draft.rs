use crate::diagnostics::RecordOrigin;
use crate::model::{CfdDictKey, CfdValue};
use crate::LoadedFormattedString;
use coflow_language::cft::{FieldName, TypeName};
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
    ResultOk(Box<ValueDraft>),
    ResultErr(Box<ValueDraft>),
    FormattedString(LoadedFormattedString),
    Object(Box<RecordDraft>),
    PendingRef {
        expected_type: TypeName,
        key: String,
    },
    Array(Vec<ValueDraft>),
    Dict(Vec<(CfdDictKey, ValueDraft)>),
}
