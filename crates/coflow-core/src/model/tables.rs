use super::ids::CfdRecordId;
use crate::schema::{RecordKey, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: TypeName,
    pub records: Vec<CfdRecordId>,
    pub primary_index: BTreeMap<RecordKey, CfdRecordId>,
}
