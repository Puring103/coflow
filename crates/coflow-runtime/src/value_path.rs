//! 中立的值路径操作；上层各自映射诊断，源码 writer 不依赖 mutation 准备层。
use crate::data_model::{CfdPathSegment, CfdValue};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathError {
    EmptyOption,
    Missing,
}
impl PathError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::EmptyOption => "cannot write a nested path through an empty option",
            Self::Missing => "pending insert nested path was not materialized",
        }
    }
}
pub(crate) fn validate(current: &CfdValue, path: &[CfdPathSegment]) -> Result<(), PathError> {
    let Some((segment, rest)) = path.split_first() else {
        return Ok(());
    };
    let next = match (current, segment) {
        (CfdValue::OptionSome(inner), _) => return validate(inner, path),
        (CfdValue::OptionNone, _) => return Err(PathError::EmptyOption),
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => {
            object.fields.get(field.as_str())
        }
        (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get(*index),
        (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
            .iter()
            .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
            .map(|(_, entry_value)| entry_value),
        _ => None,
    }
    .ok_or(PathError::Missing)?;
    validate(next, rest)
}

pub(crate) fn replace(
    current: &mut CfdValue,
    path: &[CfdPathSegment],
    value: CfdValue,
) -> Result<(), PathError> {
    let Some((segment, rest)) = path.split_first() else {
        *current = value;
        return Ok(());
    };
    match current {
        CfdValue::OptionSome(inner) => return replace(inner, path, value),
        CfdValue::OptionNone => {
            return Err(PathError::EmptyOption);
        }
        _ => {}
    }
    let next = match (current, segment) {
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => {
            object.fields.get_mut(field.as_str())
        }
        (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get_mut(*index),
        (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
            .iter_mut()
            .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
            .map(|(_, entry_value)| entry_value),
        _ => None,
    }
    .ok_or(PathError::Missing)?;
    replace(next, rest, value)
}
