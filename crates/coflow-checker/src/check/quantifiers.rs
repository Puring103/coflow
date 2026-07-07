use coflow_data_model::CfdErrorCode;

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{
    format_check_key_for_path, CheckValue, LocatedCheckValue,
};

pub(super) fn quantifier_items(
    collection: LocatedCheckValue,
) -> OpsResult<Vec<LocatedCheckValue>> {
    match collection.value {
        CheckValue::Array { items, .. } => Ok(items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                LocatedCheckValue::new(item, collection.path.clone().map(|path| path.index(index)))
            })
            .collect()),
        CheckValue::Dict { entries, .. } => Ok(entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let key_label = match format_check_key_for_path(&entry.key) {
                    Some(label) => label,
                    None => index.to_string(),
                };
                let path = collection.path.clone().map(|path| path.dict_key(key_label));
                LocatedCheckValue::new(CheckValue::Entry(Box::new(entry)), path)
            })
            .collect()),
        other => Err(OpsError::new(
            CfdErrorCode::CheckEvalTypeError,
            None,
            format!(
                "量词目标不是集合: 实际为 {}",
                format_value_for_message(&other)
            ),
        )),
    }
}
