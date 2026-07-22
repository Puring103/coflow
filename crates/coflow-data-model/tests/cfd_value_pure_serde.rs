//! `CfdValue` round-trip via serde JSON. Pins that the type is pure data —
//! no `CfdRecordId` is serialized into the wire form, so the value can be
//! shipped to external hosts (Tauri front-end, LSP, ...) without leaking
//! internal indices.

use coflow_data_model::{
    CfdDictKey, CfdEnumValue, CfdObject, CfdRecord, CfdValue, RecordCoordinate, RecordOrigin,
};
use std::collections::BTreeMap;
use std::fmt::Debug;

fn expect_eq<T: PartialEq + Debug + ?Sized>(
    actual: &T,
    expected: &T,
    context: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if actual != expected {
        return Err(std::io::Error::other(format!(
            "{context}: expected {expected:?}, got {actual:?}"
        ))
        .into());
    }
    Ok(())
}

#[test]
fn cfd_value_round_trips_through_json() -> Result<(), Box<dyn std::error::Error>> {
    let mut fields = BTreeMap::new();
    fields.insert("hp".to_string(), CfdValue::Int(42));
    fields.insert("next".to_string(), CfdValue::record_ref("fireball")?);
    fields.insert(
        "tags".to_string(),
        CfdValue::Array(vec![
            CfdValue::String("a".into()),
            CfdValue::Bool(true),
            CfdValue::Float(1.5),
        ]),
    );
    fields.insert(
        "weights".to_string(),
        CfdValue::Dict(vec![
            (CfdDictKey::String("k".into()), CfdValue::Int(1)),
            (CfdDictKey::Int(2), CfdValue::Int(2)),
        ]),
    );
    fields.insert(
        "kind".to_string(),
        CfdValue::Enum(CfdEnumValue::try_new("Element", Some("Fire"), 0)?),
    );
    fields.insert(
        "child".to_string(),
        CfdValue::Object(Box::new(CfdObject::try_new("Item", BTreeMap::new())?)),
    );

    let record = CfdRecord {
        key: RecordCoordinate::try_new("Item", "potion")?.key,
        object: CfdObject::try_new("Item", fields)?,
        origin: RecordOrigin::None,
        dimension_fields: BTreeMap::new(),
    };

    let json = serde_json::to_string(&record)?;
    if json.contains("\"target\"") {
        return Err(std::io::Error::other(format!(
            "wire should not contain old `target` id field: {json}"
        ))
        .into());
    }
    if json.contains("\"origin\"") {
        return Err(std::io::Error::other(format!(
            "wire should not contain internal origin metadata: {json}"
        ))
        .into());
    }
    if json.contains("spread_field_sources") {
        return Err(std::io::Error::other(format!(
            "wire should not contain internal spread source indexes: {json}"
        ))
        .into());
    }
    let round: CfdRecord = serde_json::from_str(&json)?;
    if record != round {
        return Err(std::io::Error::other(format!("round trip changed record: {round:?}")).into());
    }
    Ok(())
}

#[test]
fn record_coordinate_has_stable_validated_wire_form() -> Result<(), Box<dyn std::error::Error>> {
    let coordinate = RecordCoordinate::try_new("Item", "sword")?;
    let json = serde_json::to_string(&coordinate)?;
    expect_eq(
        json.as_str(),
        r#"{"actual_type":"Item","key":"sword"}"#,
        "record coordinate wire shape",
    )?;
    expect_eq(
        &serde_json::from_str::<RecordCoordinate>(&json)?,
        &coordinate,
        "record coordinate should round-trip",
    )?;

    for invalid in [
        r#"{"actual_type":"","key":"sword"}"#,
        r#"{"actual_type":"Item","key":""}"#,
        r#"{"actual_type":"not valid","key":"sword"}"#,
    ] {
        if serde_json::from_str::<RecordCoordinate>(invalid).is_ok() {
            return Err(std::io::Error::other(format!(
                "invalid coordinate should fail to deserialize: {invalid}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn cfd_i64_wire_uses_strings_and_accepts_numeric_values() -> Result<(), Box<dyn std::error::Error>>
{
    let value = CfdValue::Int(i64::MIN);
    let json = serde_json::to_string(&value)?;
    expect_eq(
        json.as_str(),
        r#"{"kind":"int","value":"-9223372036854775808"}"#,
        "i64 values should serialize as decimal strings",
    )?;
    expect_eq(
        &serde_json::from_str::<CfdValue>(&json)?,
        &value,
        "serialized i64 value should round-trip",
    )?;
    expect_eq(
        &serde_json::from_str::<CfdValue>(r#"{"kind":"int","value":42}"#)?,
        &CfdValue::Int(42),
        "numeric i64 values should deserialize",
    )?;

    expect_eq(
        &serde_json::from_str::<CfdDictKey>(r#"{"kind":"int","value":2}"#)?,
        &CfdDictKey::Int(2),
        "numeric dict keys should deserialize",
    )?;
    expect_eq(
        &serde_json::from_str::<CfdEnumValue>(
            r#"{"enum_name":"Flag","variant":null,"value":"9223372036854775807"}"#,
        )?,
        &CfdEnumValue::try_new("Flag", None::<String>, i64::MAX)?,
        "enum integer payloads should deserialize from decimal strings",
    )?;
    Ok(())
}
