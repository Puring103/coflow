//! `CfdValue` round-trip via serde JSON. Pins that the type is pure data —
//! no `CfdRecordId` is serialized into the wire form, so the value can be
//! shipped to external hosts (Tauri front-end, LSP, ...) without leaking
//! internal indices.

use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdObject, CfdRecord, CfdValue, RecordOrigin};
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
    fields.insert("next".to_string(), CfdValue::Ref("fireball".to_string()));
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
        CfdValue::Enum(CfdEnumValue {
            enum_name: "Element".into(),
            variant: Some("Fire".into()),
            value: 0,
        }),
    );
    fields.insert(
        "child".to_string(),
        CfdValue::Object(Box::new(CfdObject::new("Item", BTreeMap::new()))),
    );

    let record = CfdRecord {
        key: "potion".into(),
        object: CfdObject::new("Item", fields),
        origin: RecordOrigin::None,
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
fn cfd_i64_wire_uses_strings_and_accepts_legacy_numbers() -> Result<(), Box<dyn std::error::Error>>
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
        "legacy numeric i64 values should deserialize",
    )?;

    expect_eq(
        &serde_json::from_str::<CfdDictKey>(r#"{"kind":"int","value":2}"#)?,
        &CfdDictKey::Int(2),
        "legacy numeric dict keys should deserialize",
    )?;
    expect_eq(
        &serde_json::from_str::<CfdEnumValue>(
            r#"{"enum_name":"Flag","variant":null,"value":"9223372036854775807"}"#,
        )?,
        &CfdEnumValue {
            enum_name: "Flag".into(),
            variant: None,
            value: i64::MAX,
        },
        "enum integer payloads should deserialize from decimal strings",
    )?;
    Ok(())
}
