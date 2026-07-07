use super::ids::CfdRecordId;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: String,
    pub records: Vec<CfdRecordId>,
    pub primary_index: BTreeMap<String, CfdRecordId>,
}

/// Index of records assignable to a given root type (`abstract type` or any
/// concrete type with subclasses), keyed by record key.
///
/// The owning `CfdDataModel.inheritance_index` map keys identify the root
/// type - readers obtain it from the lookup, so it is not duplicated here.
#[derive(Debug, Clone, PartialEq)]
pub struct CfdPolymorphicIndex {
    pub records: BTreeMap<String, CfdRecordId>,
}
