use coflow_language::cft::{CftSchema, CftSchemaTypePredicate};
use coflow_model::CfdDataModel;

use super::value::EvalValue;

pub(crate) fn value_matches_predicate(
    schema: &CftSchema,
    model: &CfdDataModel,
    value: &EvalValue<'_>,
    predicate: &CftSchemaTypePredicate,
) -> bool {
    match predicate {
        CftSchemaTypePredicate::Some { .. } => matches!(
            value,
            EvalValue::Model(coflow_model::CfdValue::OptionSome(_))
                | EvalValue::Constant(coflow_language::cft::CftConstValue::OptionSome(_))
        ),
        CftSchemaTypePredicate::Type(type_name) => value
            .actual_type(model)
            .is_some_and(|actual| schema.is_assignable(actual, type_name)),
    }
}
