use coflow_data_model::{CfdDataModel, CfdErrorCode};
use coflow_structure::StructuralBudget;

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{CheckValue, LocatedBudgetExceeded, LocatedCheckValue};

pub(super) fn quantifier_len(collection: &LocatedCheckValue) -> OpsResult<usize> {
    match &collection.value {
        CheckValue::Array { items, .. } => Ok(items.len()),
        CheckValue::Dict { entries, .. } => Ok(entries.len()),
        other => Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            None,
            format!(
                "量词目标不是集合: 实际为 {}",
                format_value_for_message(other)
            ),
        )),
    }
}

pub(super) fn quantifier_item(
    collection: &LocatedCheckValue,
    index: usize,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<Option<LocatedCheckValue>> {
    match &collection.value {
        CheckValue::Array {
            items,
            element_type,
        } => items
            .located_at(
                index,
                element_type.as_ref(),
                collection.location.as_ref(),
                model,
                budget,
            )
            .map_err(budget_error),
        CheckValue::Dict {
            entries,
            key_type,
            value_type,
        } => entries
            .located_entry_at(
                index,
                key_type.as_ref(),
                value_type.as_ref(),
                collection.location.as_ref(),
                model,
                budget,
            )
            .map_err(budget_error),
        other => Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            None,
            format!(
                "量词目标不是集合: 实际为 {}",
                format_value_for_message(other)
            ),
        )),
    }
}

fn budget_error(exceeded: LocatedBudgetExceeded) -> OpsError {
    OpsError::new(
        CfdErrorCode::CheckBudgetExceeded,
        exceeded.location,
        exceeded.error.to_string(),
    )
}
