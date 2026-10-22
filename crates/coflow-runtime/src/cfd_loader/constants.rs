use crate::{LoadedDictKeyDraft, LoadedValueDraft};
use coflow_language::cft::{CftConstValue, CftSchema, CftValueType};
use coflow_language::source::Span;

use super::lower::error;
use super::{CfdTextDiagnostics, CfdTextErrorCode};

// 常量已由声明编译展开；按消费位置的类型降低，保留每层 Option 和实际对象类型。
pub(super) fn lower_constant(
    schema: &CftSchema,
    value: &CftConstValue,
    expected: &CftValueType,
    span: Span,
) -> Result<LoadedValueDraft, CfdTextDiagnostics> {
    use CftConstValue as V;
    use CftValueType as T;
    if let T::Option(inner) = expected {
        return match value {
            V::OptionNone => Ok(LoadedValueDraft::OptionNone),
            V::OptionSome(value) => lower_constant(schema, value, inner, span)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
            value => lower_constant(schema, value, inner, span)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value))),
        };
    }
    Ok(match (value, expected) {
        (V::Int(value), T::Int) => LoadedValueDraft::Int(*value),
        (V::Float(value), T::Float) => LoadedValueDraft::Float(*value),
        (V::Int(value), T::Float) => {
            #[allow(clippy::cast_precision_loss)]
            let number = *value as f64;
            LoadedValueDraft::Float(number)
        }
        (V::Bool(value), T::Bool) => LoadedValueDraft::Bool(*value),
        (V::String(value), T::String) => LoadedValueDraft::String(value.clone()),
        (
            V::Enum {
                enum_name, value, ..
            },
            T::Enum(expected),
        ) if enum_name == expected => LoadedValueDraft::enum_value(enum_name.as_str(), *value),
        (V::Array(values), T::Array(inner)) => LoadedValueDraft::Array(
            values
                .iter()
                .map(|value| lower_constant(schema, value, inner, span))
                .collect::<Result<_, _>>()?,
        ),
        (V::Dictionary(entries), T::Dict(key, item)) => LoadedValueDraft::dict(
            entries
                .iter()
                .map(|(entry_key, entry_value)| {
                    Ok((
                        lower_key(entry_key, key, span)?,
                        lower_constant(schema, entry_value, item, span)?,
                    ))
                })
                .collect::<Result<Vec<_>, CfdTextDiagnostics>>()?,
        ),
        (V::Object { type_name, fields }, T::Object(expected))
            if schema.is_assignable(type_name.as_str(), expected.as_str()) =>
        {
            let mut lowered = Vec::with_capacity(fields.len());
            for (name, value) in fields {
                let field = schema
                    .field(type_name.as_str(), name.as_str())
                    .ok_or_else(|| {
                        error(
                            CfdTextErrorCode::UnknownField,
                            format!("unknown constant field `{type_name}.{name}`"),
                            span,
                        )
                    })?;
                lowered.push((
                    name.to_string(),
                    lower_constant(schema, value, &field.value_type, span)?,
                ));
            }
            LoadedValueDraft::object(type_name.as_str(), lowered)
        }
        (V::RecordReference { type_name, id: key }, T::RecordRef(expected))
            if schema.is_assignable(type_name.as_str(), expected.as_str()) =>
        {
            LoadedValueDraft::TypedRecordRef {
                type_name: type_name.to_string(),
                id: key.clone(),
            }
        }
        _ => {
            return Err(error(
                CfdTextErrorCode::TypeMismatch,
                format!("constant does not match `{expected}`"),
                span,
            ))
        }
    })
}

fn lower_key(
    value: &CftConstValue,
    expected: &CftValueType,
    span: Span,
) -> Result<LoadedDictKeyDraft, CfdTextDiagnostics> {
    Ok(match (value, expected) {
        (CftConstValue::Int(value), CftValueType::Int) => LoadedDictKeyDraft::Int(*value),
        (CftConstValue::String(value), CftValueType::String) => {
            LoadedDictKeyDraft::String(value.clone())
        }
        (
            CftConstValue::Enum {
                enum_name, value, ..
            },
            CftValueType::Enum(expected),
        ) if enum_name == expected => LoadedDictKeyDraft::EnumValue {
            enum_name: enum_name.to_string(),
            value: *value,
        },
        _ => {
            return Err(error(
                CfdTextErrorCode::TypeMismatch,
                format!("constant key does not match `{expected}`"),
                span,
            ))
        }
    })
}
