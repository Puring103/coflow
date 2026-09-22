//! 集合编辑语义由项目层统一处理，宿主只传递操作意图。
use crate::{CfdDictKey, CfdValue, Diagnostic, DiagnosticSet};
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollectionEdit {
    ArrayAppend {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        value: Option<CfdValue>,
    },
    ArrayRemove {
        index: usize,
    },
    ArrayMove {
        from: usize,
        to: usize,
    },
    DictInsert {
        key: CfdDictKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        value: Option<CfdValue>,
    },
    DictRemove {
        key: CfdDictKey,
    },
}

/// 集合字段的纯函数编辑：单层 `Option` 包装自动展开/回包，数组/字典分支各自处理。
pub fn apply_collection_edit(
    value: CfdValue,
    edit: CollectionEdit,
    default_item: Option<CfdValue>,
) -> Result<CfdValue, DiagnosticSet> {
    match (value, edit) {
        (CfdValue::OptionSome(inner), edit) => {
            if matches!(
                inner.as_ref(),
                CfdValue::OptionSome(_) | CfdValue::OptionNone
            ) {
                return Err(collection_error("nested optional values are not supported"));
            }
            apply_collection_edit(*inner, edit, default_item)
                .map(|value| CfdValue::OptionSome(Box::new(value)))
        }
        (CfdValue::OptionNone, edit @ CollectionEdit::ArrayAppend { .. }) => {
            apply_collection_edit(CfdValue::Array(Vec::new()), edit, default_item)
                .map(|value| CfdValue::OptionSome(Box::new(value)))
        }
        (CfdValue::OptionNone, edit @ CollectionEdit::DictInsert { .. }) => {
            apply_collection_edit(CfdValue::Dict(Vec::new()), edit, default_item)
                .map(|value| CfdValue::OptionSome(Box::new(value)))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayAppend { value }) => {
            let seed = value
                .or(default_item)
                .ok_or_else(|| collection_error("array item requires an explicit value"))?;
            items.push(seed);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayRemove { index }) => {
            if index >= items.len() {
                return Err(collection_error("array index out of range"));
            }
            items.remove(index);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayMove { from, to }) => {
            if from >= items.len() || to >= items.len() {
                return Err(collection_error("array index out of range"));
            }
            if from != to {
                let moved = items.remove(from);
                items.insert(to, moved);
            }
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Dict(mut entries), CollectionEdit::DictInsert { key, value }) => {
            if entries.iter().any(|(entry_key, _)| entry_key == &key) {
                return Err(collection_error("dict key already exists"));
            }
            let seed = value
                .or(default_item)
                .ok_or_else(|| collection_error("dict value requires an explicit value"))?;
            entries.push((key, seed));
            Ok(CfdValue::Dict(entries))
        }
        (CfdValue::Dict(entries), CollectionEdit::DictRemove { key }) => {
            let original_len = entries.len();
            let entries = entries
                .into_iter()
                .filter(|(entry_key, _)| entry_key != &key)
                .collect::<Vec<_>>();
            if entries.len() == original_len {
                return Err(collection_error("dict key not found"));
            }
            Ok(CfdValue::Dict(entries))
        }
        _ => Err(collection_error(
            "collection edit target is not a collection",
        )),
    }
}

fn collection_error(message: &str) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "MUTATION-COLLECTION",
        "MUTATION",
        message,
    ))
}

#[cfg(test)]
mod collection_edit_tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn array_append_materializes_an_optional_collection() {
        let next = apply_collection_edit(
            CfdValue::OptionNone,
            CollectionEdit::ArrayAppend {
                value: Some(CfdValue::Int(1)),
            },
            None,
        )
        .expect("optional array edit");

        assert_eq!(
            next,
            CfdValue::OptionSome(Box::new(CfdValue::Array(vec![CfdValue::Int(1)])))
        );
    }
}
