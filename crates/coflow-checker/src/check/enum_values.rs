use coflow_cft::CftSchemaView;
use coflow_data_model::CfdEnumValue;

pub(super) fn enum_with_value(schema: &CftSchemaView, enum_name: &str, value: i64) -> CfdEnumValue {
    match schema.enum_value_from_int(enum_name, value) {
        Some(enum_value) => enum_value.into(),
        None => anonymous_enum_value(enum_name, value),
    }
}

pub(super) fn anonymous_enum_value(enum_name: &str, value: i64) -> CfdEnumValue {
    CfdEnumValue {
        enum_name: enum_name.to_string(),
        variant: None,
        value,
    }
}
