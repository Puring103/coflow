//! `CfdValue` round-trip via serde JSON. Pins that the type is pure data —
//! no `CfdRecordId` is serialized into the wire form, so the value can be
//! shipped to external hosts (Tauri front-end, LSP, ...) without leaking
//! internal indices.

use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdRecord, CfdValue, RecordOrigin};
use std::collections::BTreeMap;

#[test]
fn cfd_value_round_trips_through_json() {
    let mut fields = BTreeMap::new();
    fields.insert("hp".to_string(), CfdValue::Int(42));
    fields.insert(
        "next".to_string(),
        CfdValue::Ref {
            target_type: "Skill".to_string(),
            target_key: "fireball".to_string(),
        },
    );
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
        CfdValue::Object(Box::new(CfdRecord {
            key: String::new(),
            actual_type: "Item".into(),
            fields: BTreeMap::new(),
            origin: RecordOrigin::None,
            spread_field_sources: BTreeMap::new(),
        })),
    );

    let record = CfdRecord {
        key: "potion".into(),
        actual_type: "Item".into(),
        fields,
        origin: RecordOrigin::None,
        spread_field_sources: BTreeMap::new(),
    };

    let json = serde_json::to_string(&record).expect("serialize");
    assert!(
        !json.contains("\"target\""),
        "wire should not contain old `target` id field: {json}"
    );
    let round: CfdRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(record, round);
}
