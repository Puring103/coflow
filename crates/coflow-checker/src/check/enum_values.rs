use coflow_cft::{CftSchema, EnumName};
use coflow_data_model::CfdEnumValue;

pub(super) fn enum_with_value(
    schema: &CftSchema,
    enum_name: &EnumName,
    value: i64,
) -> CfdEnumValue {
    match schema.enum_value_from_int(enum_name.as_str(), value) {
        Some(enum_value) => enum_value.into(),
        None => anonymous_enum_value(enum_name, value),
    }
}

pub(super) fn anonymous_enum_value(enum_name: &EnumName, value: i64) -> CfdEnumValue {
    CfdEnumValue {
        enum_name: enum_name.clone(),
        variant: None,
        value,
    }
}
