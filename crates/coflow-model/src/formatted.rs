use crate::CfdValue;

/// 基础插值仅接受标量及已求值模板，复合值必须先选择具体字段。
///
/// # Errors
/// Returns an error when the terminal value cannot be rendered as text.
pub fn stringify_value(value: &CfdValue) -> Result<String, String> {
    Ok(match value {
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => value.to_string(),
        CfdValue::String(value) => value.clone(),
        CfdValue::FormattedString(value) => value.rendered.clone(),
        CfdValue::Enum(value) => value
            .variant
            .as_ref()
            .map_or_else(|| value.value.to_string(), ToString::to_string),
        _ => {
            return Err(
                "formatted string interpolation must end at a string, number, bool or enum field"
                    .to_string(),
            )
        }
    })
}
