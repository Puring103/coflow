use super::ids::CfdRecordHandle;
use coflow_language::cft::{RecordId, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: TypeName,
    pub records: Vec<CfdRecordHandle>,
    pub primary_index: BTreeMap<RecordId, CfdRecordHandle>,
}
