use crate::diagnostics::RecordOrigin;
use crate::model::{CfdDictKey, CfdValue};
use coflow_cft::{FieldName, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordDraft {
    pub(crate) key: String,
    pub(crate) actual_type: TypeName,
    pub(crate) fields: BTreeMap<FieldName, ValueDraft>,
    pub(crate) origin: RecordOrigin,
    pub(crate) spread_sources: Vec<SpreadFieldSource>,
    pub(crate) spread_field_sources: BTreeMap<FieldName, SpreadFieldSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SpreadFieldSource {
    pub(crate) expected_type: TypeName,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueDraft {
    Value(CfdValue),
    Object(Box<RecordDraft>),
    PendingRef {
        expected_type: TypeName,
        key: String,
    },
    PendingSpreadField {
        source_type: TypeName,
        key: String,
        field: FieldName,
    },
    Array(Vec<ValueDraft>),
    Dict(Vec<(CfdDictKey, ValueDraft)>),
    DictSpread {
        spreads: Vec<ValueDraft>,
        entries: Vec<(CfdDictKey, ValueDraft)>,
    },
}
