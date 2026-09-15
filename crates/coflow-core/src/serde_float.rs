//! JSON 使用明确的特殊值文本，二进制契约保持 IEEE 浮点表示。
use serde::{Deserialize, Deserializer, Serializer};

pub fn special(value: f64) -> Option<&'static str> {
    if value.is_nan() {
        Some("NaN")
    } else if value == f64::INFINITY {
        Some("inf")
    } else if value == f64::NEG_INFINITY {
        Some("-inf")
    } else {
        None
    }
}

pub fn parse_special(value: &str) -> Option<f64> {
    match value {
        "NaN" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if serializer.is_human_readable() {
        if let Some(text) = special(*value) {
            return serializer.serialize_str(text);
        }
    }
    serializer.serialize_f64(*value)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    if !deserializer.is_human_readable() {
        return f64::deserialize(deserializer);
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Number(f64),
        Special(String),
    }
    match Wire::deserialize(deserializer)? {
        Wire::Number(value) => Ok(value),
        Wire::Special(text) => parse_special(&text)
            .ok_or_else(|| serde::de::Error::custom("expected float or inf, -inf, NaN")),
    }
}
