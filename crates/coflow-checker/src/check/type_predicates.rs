use coflow_cft::{CftSchemaTypePredicate, CftSchemaView};
use coflow_data_model::CfdDataModel;

use super::value::CheckValue;

pub(super) fn value_matches_predicate(
    schema: &CftSchemaView,
    model: &CfdDataModel,
    value: &CheckValue,
    predicate: &CftSchemaTypePredicate,
) -> bool {
    match predicate {
        CftSchemaTypePredicate::Null => matches!(value, CheckValue::Null),
        CftSchemaTypePredicate::Type(type_name) => value
            .actual_type(model)
            .is_some_and(|actual| schema.is_assignable(actual, type_name)),
    }
}
