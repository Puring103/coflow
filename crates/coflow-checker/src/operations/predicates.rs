use coflow_cft::{CftSchema, CftSchemaTypePredicate};
use coflow_data_model::CfdDataModel;

use super::value::{EvalValue, ScalarValue};

pub(crate) fn value_matches_predicate(
    schema: &CftSchema,
    model: &CfdDataModel,
    value: &EvalValue<'_>,
    predicate: &CftSchemaTypePredicate,
) -> bool {
    match predicate {
        CftSchemaTypePredicate::Null => matches!(value.scalar(), Some(ScalarValue::Null)),
        CftSchemaTypePredicate::Type(type_name) => value
            .actual_type(model)
            .is_some_and(|actual| schema.is_assignable(actual, type_name)),
    }
}
